use std::env;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

use tar::{Archive, EntryType};
use xz2::read::XzDecoder;

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
        Some("--help") | Some("-h") | None => {
            print_help();
            Ok(())
        }
        Some(command) => Err(format!("unknown command: {command}")),
    }
}

fn print_help() {
    println!("bluefield-validate");
    println!();
    println!("Commands:");
    println!(
        "  check-kexec-tarball <tarball>  Check trusted nixos-anywhere kexec tarball metadata"
    );
    println!("  build-bfb [options]             Build a BlueField BFB from a kexec tarball");
    println!();
    println!("build-bfb options:");
    println!("  --base-bfb <path>        Compatible NVIDIA BFB carrier");
    println!("  --kexec-tarball <path>   Trusted nixos-anywhere kexec tarball");
    println!("  --output <path>          Output BFB path");
    println!("  --cmdline <string>       Override kernel command line");
    println!("  --description <string>   BFB boot menu description");
    println!("  --mlx-mkbfb <path>       mlx-mkbfb executable (default: mlx-mkbfb)");
    println!("  --keep-workdir           Keep temporary extracted payload files");
}

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

fn required_option_value(
    iter: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, String> {
    iter.next()
        .ok_or_else(|| format!("missing value for {option}\n{}", build_bfb_usage()))
}

fn build_bfb_usage() -> String {
    "usage: bluefield-validate build-bfb --base-bfb <path> --kexec-tarball <path> --output <path> [--cmdline <string>] [--description <string>] [--mlx-mkbfb <path>] [--keep-workdir]".to_string()
}

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

struct TempWorkdir {
    path: PathBuf,
    keep: bool,
}

impl TempWorkdir {
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

    fn path(&self) -> &Path {
        &self.path
    }

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

#[derive(Debug)]
struct TarEntry {
    path: PathBuf,
    entry_type: EntryType,
}

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

fn validate_entry_type(entry_type: EntryType, path: &Path) -> Result<(), String> {
    if entry_type.is_file() || entry_type.is_dir() {
        return Ok(());
    }

    Err(format!(
        "unsafe tar entry type for {}; only regular files and directories are allowed",
        path.display()
    ))
}

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

fn normalized_relative_path(path: &Path) -> Result<PathBuf, String> {
    validate_relative_path(path)?;
    Ok(path.components().collect())
}

fn validate_owner(uid: u64, gid: u64, path: &Path) -> Result<(), String> {
    if uid != 0 || gid != 0 {
        return Err(format!(
            "unsafe non-root ownership for {}: {uid}:{gid}",
            path.display()
        ));
    }

    Ok(())
}

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
