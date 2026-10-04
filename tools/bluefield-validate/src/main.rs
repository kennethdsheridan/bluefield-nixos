//! Validation and build helper for NixOS-on-BlueField recovery artifacts.
//!
//! The binary has two responsibilities:
//! - validate the restricted `nixos-anywhere` kexec tarball layout expected by
//!   this flake;
//! - wrap NVIDIA's `mlx-mkbfb` tool to place that kexec payload into a
//!   compatible carrier BFB;
//! - install a BFB through RShim in a way that can recover from missing
//!   unprivileged access to `/dev/rshim*` by re-executing itself through `sudo`.
//! - restore the host-side tmfifo address after interrupted RShim boot attempts.
//!
//! The checks are intentionally conservative because the resulting artifacts are
//! used during recovery and install flows where a malformed payload can make the
//! DPU unreachable over tmfifo.

use std::env;
use std::fs::{self, File};
use std::io::BufReader;
use std::os::raw::c_int;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tar::{Archive, EntryType};
use xz2::read::XzDecoder;

const SIGTERM: c_int = 15;
const SIGKILL: c_int = 9;

extern "C" {
    fn kill(pid: c_int, sig: c_int) -> c_int;
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("check-kexec-tarball") => {
            let path = args
                .next()
                .ok_or("usage: bluefield-validate check-kexec-tarball <tarball>")?;
            check_kexec_tarball(Path::new(&path))
        }
        Some("build-bfb") => {
            let args: Vec<String> = args.collect();
            if args.iter().any(|arg| arg == "--help" || arg == "-h") {
                println!("{}", build_bfb_usage());
                Ok(())
            } else {
                build_bfb(BuildBfbArgs::parse(args)?)
            }
        }
        Some("install-bfb") => {
            let args: Vec<String> = args.collect();
            if args.iter().any(|arg| arg == "--help" || arg == "-h") {
                println!("{}", install_bfb_usage());
                Ok(())
            } else {
                install_bfb(InstallBfbArgs::parse(args.clone())?, args)
            }
        }
        Some("repair-host-tmfifo") => {
            let args: Vec<String> = args.collect();
            if args.iter().any(|arg| arg == "--help" || arg == "-h") {
                println!("{}", repair_host_tmfifo_usage());
                Ok(())
            } else {
                repair_host_tmfifo(RepairHostTmfifoArgs::parse(args.clone())?, args)
            }
        }
        Some("--help") | Some("-h") | None => {
            print_help();
            Ok(())
        }
        Some(command) => Err(format!("unknown command: {command}")),
    }
}

/// Prints the top-level command help for the small hand-rolled CLI.
fn print_help() {
    println!("bluefield-validate");
    println!();
    println!("Commands:");
    println!(
        "  check-kexec-tarball <tarball>  Check trusted nixos-anywhere kexec tarball metadata"
    );
    println!("  build-bfb [options]             Build a BlueField BFB from a kexec tarball");
    println!(
        "  install-bfb [options]           Install a BFB over local RShim with sudo self-heal"
    );
    println!("  repair-host-tmfifo [options]    Restore host-side tmfifo IPv4 settings");
    println!();
    println!("build-bfb options:");
    println!("  --base-bfb <path>        Compatible NVIDIA BFB carrier");
    println!("  --kexec-tarball <path>   Trusted nixos-anywhere kexec tarball");
    println!("  --output <path>          Output BFB path");
    println!("  --cmdline <string>       Override kernel command line");
    println!("  --description <string>   BFB boot menu description");
    println!("  --mlx-mkbfb <path>       mlx-mkbfb executable (default: mlx-mkbfb)");
    println!("  --keep-workdir           Keep temporary extracted payload files");
    println!();
    println!("install-bfb options:");
    println!("  --bfb <path>             BFB image to stream through RShim");
    println!("  --rshim <device>         RShim device name (default: rshim0)");
    println!("  --bfb-install <path>     bfb-install executable (default: bfb-install)");
    println!("  --sudo <path>            sudo executable for privilege self-heal (default: sudo)");
    println!("  --keep-log               Preserve bfb-install log output");
    println!("  --verbose                Enable verbose bfb-install output");
    println!("  --timeout-seconds <n>    Stop bfb-install if it hangs (default: 900)");
    println!("  --dry-run                Print the install command without streaming the BFB");
    println!();
    println!("repair-host-tmfifo options:");
    println!("  --interface <name>       Host tmfifo interface (default: tmfifo_net0)");
    println!("  --address <cidr>         Host tmfifo address (default: 192.168.100.1/30)");
    println!("  --ip <path>              ip executable (default: ip)");
    println!("  --sudo <path>            sudo executable for privilege self-heal (default: sudo)");
    println!("  --dry-run                Print commands without changing networking");
}

/// Parsed arguments for restoring the host side of the tmfifo link.
struct RepairHostTmfifoArgs {
    interface: String,
    address: String,
    ip: PathBuf,
    sudo: PathBuf,
    dry_run: bool,
}

impl RepairHostTmfifoArgs {
    /// Parses `repair-host-tmfifo` options.
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut interface = "tmfifo_net0".to_string();
        let mut address = "192.168.100.1/30".to_string();
        let mut ip = PathBuf::from("ip");
        let mut sudo = default_sudo_path();
        let mut dry_run = false;
        let mut iter = args.into_iter();

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--interface" => {
                    interface = required_repair_option_value(&mut iter, "--interface")?
                }
                "--address" => address = required_repair_option_value(&mut iter, "--address")?,
                "--ip" => ip = required_repair_option_value(&mut iter, "--ip")?.into(),
                "--sudo" => sudo = required_repair_option_value(&mut iter, "--sudo")?.into(),
                "--dry-run" => dry_run = true,
                "--help" | "-h" => return Err(repair_host_tmfifo_usage()),
                _ => {
                    return Err(format!(
                        "unknown repair-host-tmfifo option: {arg}\n{}",
                        repair_host_tmfifo_usage()
                    ))
                }
            }
        }

        Ok(Self {
            interface,
            address,
            ip,
            sudo,
            dry_run,
        })
    }
}

/// Reads a required `repair-host-tmfifo` option value from the argument iterator.
fn required_repair_option_value(
    iter: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, String> {
    iter.next()
        .ok_or_else(|| format!("missing value for {option}\n{}", repair_host_tmfifo_usage()))
}

/// Returns the one-line `repair-host-tmfifo` usage string used in parse errors.
fn repair_host_tmfifo_usage() -> String {
    "usage: bluefield-validate repair-host-tmfifo [--interface <name>] [--address <cidr>] [--ip <path>] [--sudo <path>] [--dry-run]".to_string()
}

/// Restores the host-side tmfifo address and link state, using sudo when needed.
fn repair_host_tmfifo(args: RepairHostTmfifoArgs, raw_args: Vec<String>) -> Result<(), String> {
    if args.dry_run {
        println!(
            "would run: {} address replace {} dev {}",
            args.ip.display(),
            args.address,
            args.interface
        );
        println!(
            "would run: {} link set {} up",
            args.ip.display(),
            args.interface
        );
        return Ok(());
    }

    match run_host_tmfifo_repair(&args) {
        Ok(()) => {
            println!("ok: restored {} on {}", args.address, args.interface);
            Ok(())
        }
        Err(error) if env::var_os("BLUEFIELD_VALIDATE_REPAIR_SUDO_REEXEC").is_none() => {
            sudo_reexec_repair_host_tmfifo(&args, &raw_args, &error)
        }
        Err(error) => Err(error),
    }
}

/// Applies the host-side tmfifo address and brings the link up.
fn run_host_tmfifo_repair(args: &RepairHostTmfifoArgs) -> Result<(), String> {
    run_ip_command(
        &args.ip,
        &["address", "replace", &args.address, "dev", &args.interface],
    )?;
    run_ip_command(&args.ip, &["link", "set", &args.interface, "up"])
}

/// Runs one `ip` command and returns a contextual error on failure.
fn run_ip_command(ip: &Path, args: &[&str]) -> Result<(), String> {
    let status = Command::new(ip)
        .args(args)
        .status()
        .map_err(|error| format!("failed to run {}: {error}", ip.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} {} failed with status {status}",
            ip.display(),
            args.join(" ")
        ))
    }
}

/// Re-executes tmfifo host repair through sudo while preserving the Nix wrapper PATH.
fn sudo_reexec_repair_host_tmfifo(
    args: &RepairHostTmfifoArgs,
    raw_args: &[String],
    original_error: &str,
) -> Result<(), String> {
    let current_exe = env::current_exe().map_err(|error| {
        format!("failed to resolve current executable for sudo re-exec: {error}")
    })?;
    let path = env::var_os("PATH").unwrap_or_default();
    let status = Command::new(&args.sudo)
        .arg("env")
        .arg("BLUEFIELD_VALIDATE_REPAIR_SUDO_REEXEC=1")
        .arg(format!("PATH={}", path.to_string_lossy()))
        .arg(current_exe)
        .arg("repair-host-tmfifo")
        .args(raw_args)
        .status()
        .map_err(|error| {
            format!(
                "failed to run {} for sudo re-exec: {error}",
                args.sudo.display()
            )
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} re-exec failed with status {status}; original error: {original_error}",
            args.sudo.display()
        ))
    }
}

/// Prefers the NixOS setuid sudo wrapper over a non-setuid store sudo binary.
fn default_sudo_path() -> PathBuf {
    let nixos_wrapper = Path::new("/run/wrappers/bin/sudo");
    if nixos_wrapper.exists() {
        nixos_wrapper.to_path_buf()
    } else {
        PathBuf::from("sudo")
    }
}

/// Parsed arguments for `bluefield-validate install-bfb`.
struct InstallBfbArgs {
    bfb: PathBuf,
    rshim: String,
    bfb_install: PathBuf,
    sudo: PathBuf,
    keep_log: bool,
    verbose: bool,
    timeout_seconds: u64,
    dry_run: bool,
}

impl InstallBfbArgs {
    /// Parses `install-bfb` options while preserving unknown-option failures.
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut bfb = None;
        let mut rshim = "rshim0".to_string();
        let mut bfb_install = PathBuf::from("bfb-install");
        let mut sudo = default_sudo_path();
        let mut keep_log = false;
        let mut verbose = false;
        let mut timeout_seconds = 900;
        let mut dry_run = false;
        let mut iter = args.into_iter();

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--bfb" => bfb = Some(required_install_option_value(&mut iter, "--bfb")?.into()),
                "--rshim" => rshim = required_install_option_value(&mut iter, "--rshim")?,
                "--bfb-install" => {
                    bfb_install = required_install_option_value(&mut iter, "--bfb-install")?.into()
                }
                "--sudo" => sudo = required_install_option_value(&mut iter, "--sudo")?.into(),
                "--keep-log" => keep_log = true,
                "--verbose" => verbose = true,
                "--timeout-seconds" => {
                    timeout_seconds = required_install_option_value(&mut iter, "--timeout-seconds")?
                        .parse()
                        .map_err(|error| format!("invalid --timeout-seconds value: {error}"))?
                }
                "--dry-run" => dry_run = true,
                "--help" | "-h" => return Err(install_bfb_usage()),
                _ => {
                    return Err(format!(
                        "unknown install-bfb option: {arg}\n{}",
                        install_bfb_usage()
                    ))
                }
            }
        }

        Ok(Self {
            bfb: bfb.ok_or_else(install_bfb_usage)?,
            rshim,
            bfb_install,
            sudo,
            keep_log,
            verbose,
            timeout_seconds,
            dry_run,
        })
    }
}

/// Reads a required `install-bfb` option value from the argument iterator.
fn required_install_option_value(
    iter: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, String> {
    iter.next()
        .ok_or_else(|| format!("missing value for {option}\n{}", install_bfb_usage()))
}

/// Returns the one-line `install-bfb` usage string used in parse errors and help output.
fn install_bfb_usage() -> String {
    "usage: bluefield-validate install-bfb --bfb <path> [--rshim <device>] [--bfb-install <path>] [--sudo <path>] [--keep-log] [--verbose] [--timeout-seconds <n>] [--dry-run]".to_string()
}

/// Installs a BFB over RShim, re-executing through sudo if local RShim nodes are protected.
fn install_bfb(args: InstallBfbArgs, raw_args: Vec<String>) -> Result<(), String> {
    if !args.bfb.is_file() {
        return Err(format!("BFB does not exist: {}", args.bfb.display()));
    }

    let rshim_arg = rshim_argument(&args.rshim)?;
    let command_args = bfb_install_args(&args, &rshim_arg);
    if args.dry_run {
        println!(
            "would run: {} {}",
            args.bfb_install.display(),
            command_args.join(" ")
        );
        return Ok(());
    }

    let rshim_dir = rshim_device_dir(&args.rshim)?;
    let misc = rshim_dir.join("misc");
    let boot = rshim_dir.join("boot");
    if !misc.exists() || !boot.exists() {
        return Err(format!(
            "RShim device is missing required nodes under {}",
            rshim_dir.display()
        ));
    }

    if !rshim_accessible(&misc, &boot) {
        return sudo_reexec_install_bfb(&args, &raw_args, &rshim_dir);
    }

    let mut command = Command::new(&args.bfb_install);
    command.args(&command_args);
    let status = run_with_timeout(&mut command, Duration::from_secs(args.timeout_seconds))
        .map_err(|error| format!("failed to run {}: {error}", args.bfb_install.display()))?;
    if !status.success() {
        return Err(format!(
            "{} failed with status {status}",
            args.bfb_install.display()
        ));
    }

    println!("ok: streamed {} to {rshim_arg}", args.bfb.display());
    Ok(())
}

/// Builds the device directory path for a local RShim argument.
fn rshim_device_dir(rshim: &str) -> Result<PathBuf, String> {
    if rshim.contains(':') {
        return Err("install-bfb self-heal supports local RShim devices only".to_string());
    }

    if rshim.starts_with("/dev/") {
        Ok(PathBuf::from(rshim))
    } else {
        Ok(Path::new("/dev").join(rshim))
    }
}

/// Converts `/dev/rshimN` into the `rshimN` argument expected by bfb-install.
fn rshim_argument(rshim: &str) -> Result<String, String> {
    if rshim.starts_with("/dev/") {
        Path::new(rshim)
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
            .ok_or_else(|| format!("invalid RShim device path: {rshim}"))
    } else {
        Ok(rshim.to_string())
    }
}

/// Checks whether the current process can read RShim status and open the boot node for writing.
fn rshim_accessible(misc: &Path, boot: &Path) -> bool {
    File::open(misc).is_ok() && fs::OpenOptions::new().write(true).open(boot).is_ok()
}

/// Re-executes this helper through sudo instead of relying on bfb-install's internal sudo lookup.
fn sudo_reexec_install_bfb(
    args: &InstallBfbArgs,
    raw_args: &[String],
    rshim_dir: &Path,
) -> Result<(), String> {
    if env::var_os("BLUEFIELD_VALIDATE_SUDO_REEXEC").is_some() {
        return Err(format!(
            "RShim nodes under {} are still inaccessible after sudo re-exec",
            rshim_dir.display()
        ));
    }

    let current_exe = env::current_exe().map_err(|error| {
        format!("failed to resolve current executable for sudo re-exec: {error}")
    })?;
    let path = env::var_os("PATH").unwrap_or_default();
    let mut command = Command::new(&args.sudo);
    command
        .arg("env")
        .arg("BLUEFIELD_VALIDATE_SUDO_REEXEC=1")
        .arg(format!("PATH={}", path.to_string_lossy()))
        .arg(current_exe)
        .arg("install-bfb")
        .args(raw_args);

    if args.dry_run {
        println!(
            "would re-exec through sudo for protected {} using {}",
            rshim_dir.display(),
            args.sudo.display()
        );
        return Ok(());
    }

    let status = run_with_timeout(&mut command, Duration::from_secs(args.timeout_seconds + 60))
        .map_err(|error| {
            format!(
                "failed to run {} for sudo re-exec: {error}",
                args.sudo.display()
            )
        })?;
    if !status.success() {
        return Err(format!(
            "{} re-exec failed with status {status}",
            args.sudo.display()
        ));
    }

    Ok(())
}

/// Builds the `bfb-install` argv used after access preflight and optional sudo re-exec.
fn bfb_install_args(args: &InstallBfbArgs, rshim_arg: &str) -> Vec<String> {
    let mut command_args = vec![
        "-r".to_string(),
        rshim_arg.to_string(),
        "-b".to_string(),
        args.bfb.display().to_string(),
    ];
    if args.keep_log {
        command_args.push("-k".to_string());
    }
    if args.verbose {
        command_args.push("-v".to_string());
    }
    command_args
}

/// Runs a child process with a bounded wait and kills it if it stops making progress forever.
fn run_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<std::process::ExitStatus, String> {
    command.process_group(0);

    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to spawn child process: {error}"))?;
    let start = Instant::now();

    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("failed to poll child process: {error}"))?
        {
            return Ok(status);
        }

        if start.elapsed() >= timeout {
            let _ = signal_process_group(child.id(), SIGTERM);
            if !wait_for_child(&mut child, Duration::from_secs(5))? {
                let _ = signal_process_group(child.id(), SIGKILL);
                let _ = child.wait();
            }
            return Err(format!(
                "child process group exceeded {}s timeout",
                timeout.as_secs()
            ));
        }

        thread::sleep(Duration::from_secs(1));
    }
}

/// Waits a short grace period for a child to exit after timeout signaling.
fn wait_for_child(child: &mut std::process::Child, timeout: Duration) -> Result<bool, String> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if child
            .try_wait()
            .map_err(|error| format!("failed to poll child process after timeout: {error}"))?
            .is_some()
        {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(false)
}

/// Sends a signal to the child's whole process group so shell pipelines do not survive timeouts.
fn signal_process_group(child_id: u32, signal: c_int) -> Result<(), String> {
    let pgid: c_int = child_id
        .try_into()
        .map_err(|_| format!("child process id {child_id} does not fit in pid_t"))?;
    let result = unsafe { kill(-pgid, signal) };
    if result == 0 {
        Ok(())
    } else {
        Err(format!(
            "failed to signal child process group {pgid}: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_with_timeout_kills_shell_pipeline_process_group() {
        let marker = env::temp_dir().join(format!(
            "bluefield-validate-timeout-child-{}.pid",
            std::process::id()
        ));
        let _ = fs::remove_file(&marker);

        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("sleep 30 & printf '%s\\n' \"$!\" > \"$1\"; wait")
            .arg("bluefield-timeout-test")
            .arg(&marker);

        let result = run_with_timeout(&mut command, Duration::from_secs(1));
        assert!(result.is_err());

        let child_pid: c_int = fs::read_to_string(&marker)
            .expect("shell should write background child pid before timeout")
            .trim()
            .parse()
            .expect("background child pid should parse");

        for _ in 0..20 {
            if !process_exists(child_pid) {
                let _ = fs::remove_file(&marker);
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }

        let _ = unsafe { kill(child_pid, SIGKILL) };
        let _ = fs::remove_file(&marker);
        panic!("background child survived process-group timeout cleanup");
    }

    fn process_exists(pid: c_int) -> bool {
        let result = unsafe { kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(3)
    }
}

/// Validates that a tarball contains the exact safe kexec layout this flake consumes.
fn check_kexec_tarball(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("tarball does not exist: {}", path.display()));
    }

    let entries = read_tar_entries(path)?;
    let required = ["kexec/run", "kexec/bzImage", "kexec/initrd.gz"];
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|required_entry| {
            !entries
                .iter()
                .any(|entry| entry.path == Path::new(required_entry) && entry.entry_type.is_file())
        })
        .collect();

    if !missing.is_empty() {
        return Err(format!("missing required entries: {}", missing.join(", ")));
    }

    println!(
        "ok: {} contains nixos-anywhere kexec layout",
        path.display()
    );
    Ok(())
}

/// Parsed arguments for `bluefield-validate build-bfb`.
struct BuildBfbArgs {
    base_bfb: PathBuf,
    kexec_tarball: PathBuf,
    output: PathBuf,
    cmdline: Option<String>,
    description: String,
    mlx_mkbfb: PathBuf,
    keep_workdir: bool,
}

impl BuildBfbArgs {
    /// Parses `build-bfb` options without pulling in a CLI dependency for this small tool.
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut base_bfb = None;
        let mut kexec_tarball = None;
        let mut output = None;
        let mut cmdline = None;
        let mut description = "NixOS BlueField installer".to_string();
        let mut mlx_mkbfb = PathBuf::from("mlx-mkbfb");
        let mut keep_workdir = false;
        let mut iter = args.into_iter();

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--base-bfb" => {
                    base_bfb = Some(required_option_value(&mut iter, "--base-bfb")?.into())
                }
                "--kexec-tarball" => {
                    kexec_tarball =
                        Some(required_option_value(&mut iter, "--kexec-tarball")?.into())
                }
                "--output" => output = Some(required_option_value(&mut iter, "--output")?.into()),
                "--cmdline" => cmdline = Some(required_option_value(&mut iter, "--cmdline")?),
                "--description" => description = required_option_value(&mut iter, "--description")?,
                "--mlx-mkbfb" => {
                    mlx_mkbfb = required_option_value(&mut iter, "--mlx-mkbfb")?.into()
                }
                "--keep-workdir" => keep_workdir = true,
                "--help" | "-h" => return Err(build_bfb_usage()),
                _ => {
                    return Err(format!(
                        "unknown build-bfb option: {arg}\n{}",
                        build_bfb_usage()
                    ))
                }
            }
        }

        Ok(Self {
            base_bfb: base_bfb.ok_or_else(build_bfb_usage)?,
            kexec_tarball: kexec_tarball.ok_or_else(build_bfb_usage)?,
            output: output.ok_or_else(build_bfb_usage)?,
            cmdline,
            description,
            mlx_mkbfb,
            keep_workdir,
        })
    }
}

/// Reads a required option value from the argument iterator.
fn required_option_value(
    iter: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, String> {
    iter.next()
        .ok_or_else(|| format!("missing value for {option}\n{}", build_bfb_usage()))
}

/// Returns the one-line `build-bfb` usage string used in parse errors and help output.
fn build_bfb_usage() -> String {
    "usage: bluefield-validate build-bfb --base-bfb <path> --kexec-tarball <path> --output <path> [--cmdline <string>] [--description <string>] [--mlx-mkbfb <path>] [--keep-workdir]".to_string()
}

/// Builds a custom BlueField BFB from a validated kexec tarball and carrier BFB.
fn build_bfb(args: BuildBfbArgs) -> Result<(), String> {
    if !args.base_bfb.is_file() {
        return Err(format!(
            "base BFB does not exist: {}",
            args.base_bfb.display()
        ));
    }
    if let Some(parent) = args.output.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(format!(
                "output directory does not exist: {}",
                parent.display()
            ));
        }
    }

    check_kexec_tarball(&args.kexec_tarball)?;

    let workdir = TempWorkdir::create("bluefield-bfb")?;
    let image_path = workdir.path().join("Image");
    let initrd_path = workdir.path().join("initrd.gz");
    let run_path = workdir.path().join("run");

    extract_kexec_payload(&args.kexec_tarball, &image_path, &initrd_path, &run_path)?;
    let cmdline = match args.cmdline {
        Some(cmdline) => cmdline,
        None => parse_kexec_cmdline(&fs::read_to_string(&run_path).map_err(|error| {
            format!("failed to read extracted {}: {error}", run_path.display())
        })?)?,
    };

    run_mlx_mkbfb(
        &args.mlx_mkbfb,
        &args.base_bfb,
        &image_path,
        &initrd_path,
        &cmdline,
        &args.description,
        &args.output,
    )?;
    run_mlx_mkbfb_check(&args.mlx_mkbfb, &args.output)?;

    if args.keep_workdir {
        println!("kept workdir: {}", workdir.keep().display());
    }
    println!("ok: wrote BFB {}", args.output.display());
    Ok(())
}

/// Extracts the trusted kexec payload members into a temporary working directory.
fn extract_kexec_payload(
    tarball: &Path,
    image_path: &Path,
    initrd_path: &Path,
    run_path: &Path,
) -> Result<(), String> {
    let file = File::open(tarball)
        .map_err(|error| format!("failed to open {}: {error}", tarball.display()))?;
    let reader = BufReader::new(file);
    let decoder = XzDecoder::new(reader);
    let mut archive = Archive::new(decoder);
    let mut found_run = false;
    let mut found_image = false;
    let mut found_initrd = false;

    for entry in archive
        .entries()
        .map_err(|error| format!("failed to read archive entries: {error}"))?
    {
        let mut entry = entry.map_err(|error| format!("failed to read archive entry: {error}"))?;
        let path = entry
            .path()
            .map_err(|error| format!("failed to read archive path: {error}"))?
            .into_owned();
        let path = normalized_relative_path(&path)?;

        let destination = if path == Path::new("kexec/bzImage") {
            found_image = true;
            Some(image_path)
        } else if path == Path::new("kexec/initrd.gz") {
            found_initrd = true;
            Some(initrd_path)
        } else if path == Path::new("kexec/run") {
            found_run = true;
            Some(run_path)
        } else {
            None
        };

        if let Some(destination) = destination {
            let mut output = File::create(destination)
                .map_err(|error| format!("failed to create {}: {error}", destination.display()))?;
            std::io::copy(&mut entry, &mut output)
                .map_err(|error| format!("failed to extract {}: {error}", path.display()))?;
        }
    }

    if !(found_run && found_image && found_initrd) {
        return Err("failed to extract complete kexec payload".to_string());
    }

    Ok(())
}

/// Extracts the kernel command line from the nixos-anywhere generated `kexec/run` script.
fn parse_kexec_cmdline(run_script: &str) -> Result<String, String> {
    let marker = "--command-line \"";
    let start = run_script
        .find(marker)
        .ok_or("kexec/run does not contain --command-line; pass --cmdline explicitly")?
        + marker.len();
    let rest = &run_script[start..];
    let end = rest
        .find('"')
        .ok_or("kexec/run has an unterminated --command-line value")?;
    Ok(rest[..end].to_string())
}

/// Invokes `mlx-mkbfb` to combine the carrier BFB with the custom boot payload.
fn run_mlx_mkbfb(
    mlx_mkbfb: &Path,
    base_bfb: &Path,
    image_path: &Path,
    initrd_path: &Path,
    cmdline: &str,
    description: &str,
    output: &Path,
) -> Result<(), String> {
    let status = Command::new(mlx_mkbfb)
        .arg(base_bfb)
        .arg(format!("--image={}", image_path.display()))
        .arg(format!("--initramfs={}", initrd_path.display()))
        // mlx-mkbfb uses the double equals form for values that can contain
        // additional `=` characters, especially full kernel command lines.
        .arg(format!("--boot-args=={cmdline}"))
        .arg(format!("--boot-desc=={description}"))
        .arg(output)
        .status()
        .map_err(|error| format!("failed to run {}: {error}", mlx_mkbfb.display()))?;

    if !status.success() {
        return Err(format!(
            "{} failed with status {status}",
            mlx_mkbfb.display()
        ));
    }

    Ok(())
}

/// Verifies the generated BFB using `mlx-mkbfb -c` before reporting success.
fn run_mlx_mkbfb_check(mlx_mkbfb: &Path, output: &Path) -> Result<(), String> {
    let status = Command::new(mlx_mkbfb)
        .arg("-c")
        .arg(output)
        .status()
        .map_err(|error| format!("failed to run {} -c: {error}", mlx_mkbfb.display()))?;

    if !status.success() {
        return Err(format!(
            "{} -c failed for {} with status {status}",
            mlx_mkbfb.display(),
            output.display()
        ));
    }

    Ok(())
}

/// Temporary work directory that removes itself unless explicitly kept for debugging.
struct TempWorkdir {
    path: PathBuf,
    keep: bool,
}

impl TempWorkdir {
    /// Creates a unique temporary directory under the process temp directory.
    fn create(prefix: &str) -> Result<Self, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("system clock is before UNIX_EPOCH: {error}"))?
            .as_nanos();
        let path = env::temp_dir().join(format!("{prefix}-{}-{now}", std::process::id()));
        fs::create_dir(&path)
            .map_err(|error| format!("failed to create temp dir {}: {error}", path.display()))?;
        Ok(Self { path, keep: false })
    }

    /// Returns the temporary directory path.
    fn path(&self) -> &Path {
        &self.path
    }

    /// Marks the directory as retained and returns its path for operator inspection.
    fn keep(mut self) -> PathBuf {
        self.keep = true;
        self.path.clone()
    }
}

impl Drop for TempWorkdir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// Sanitized metadata for one tar entry.
#[derive(Debug)]
struct TarEntry {
    path: PathBuf,
    entry_type: EntryType,
}

/// Reads and validates tar entry metadata without extracting untrusted paths first.
fn read_tar_entries(path: &Path) -> Result<Vec<TarEntry>, String> {
    let file =
        File::open(path).map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    let reader = BufReader::new(file);
    let decoder = XzDecoder::new(reader);
    let mut archive = Archive::new(decoder);
    let mut entries = Vec::new();

    for entry in archive
        .entries()
        .map_err(|error| format!("failed to read archive entries: {error}"))?
    {
        let entry = entry.map_err(|error| format!("failed to read archive entry: {error}"))?;
        let header = entry.header();
        let entry_type = header.entry_type();
        let path = entry
            .path()
            .map_err(|error| format!("failed to read archive path: {error}"))?
            .into_owned();
        let path = normalized_relative_path(&path)?;

        validate_entry_type(entry_type, &path)?;
        validate_relative_path(&path)?;
        validate_owner(
            header.uid().unwrap_or(u64::MAX),
            header.gid().unwrap_or(u64::MAX),
            &path,
        )?;
        validate_mode(header.mode().unwrap_or(u32::MAX), entry_type, &path)?;

        if entries
            .iter()
            .any(|existing: &TarEntry| existing.path == path)
        {
            return Err(format!("duplicate tar entry: {}", path.display()));
        }

        entries.push(TarEntry { path, entry_type });
    }

    Ok(entries)
}

/// Allows only regular files and directories in recovery tarballs.
fn validate_entry_type(entry_type: EntryType, path: &Path) -> Result<(), String> {
    if entry_type.is_file() || entry_type.is_dir() {
        return Ok(());
    }

    Err(format!(
        "unsafe tar entry type for {}; only regular files and directories are allowed",
        path.display()
    ))
}

/// Rejects absolute, parent-relative, and otherwise non-normal tar paths.
fn validate_relative_path(path: &Path) -> Result<(), String> {
    if path.is_absolute() {
        return Err(format!("unsafe absolute tar path: {}", path.display()));
    }

    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => return Err(format!("unsafe tar path: {}", path.display())),
        }
    }

    Ok(())
}

/// Normalizes a tar path after proving it is relative and component-safe.
fn normalized_relative_path(path: &Path) -> Result<PathBuf, String> {
    validate_relative_path(path)?;
    Ok(path.components().collect())
}

/// Requires root-owned tar entries so user-built archives cannot smuggle unsafe metadata.
fn validate_owner(uid: u64, gid: u64, path: &Path) -> Result<(), String> {
    if uid != 0 || gid != 0 {
        return Err(format!(
            "unsafe non-root ownership for {}: {uid}:{gid}",
            path.display()
        ));
    }

    Ok(())
}

/// Rejects special or writable modes and enforces executable/searchable bits where needed.
fn validate_mode(mode: u32, entry_type: EntryType, path: &Path) -> Result<(), String> {
    if mode & 0o7000 != 0 {
        return Err(format!(
            "unsafe special mode bits on {}: {mode:o}",
            path.display()
        ));
    }

    if mode & 0o022 != 0 {
        return Err(format!(
            "unsafe writable mode on {}: {mode:o}",
            path.display()
        ));
    }

    if path == Path::new("kexec/run") && mode & 0o111 == 0 {
        return Err("kexec/run is not executable in the tarball".to_string());
    }

    if entry_type.is_dir() && mode & 0o111 == 0 {
        return Err(format!("directory is not searchable: {}", path.display()));
    }

    Ok(())
}
