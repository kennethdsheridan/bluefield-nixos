use std::env;
use std::path::Path;
use std::process::{Command, ExitCode};

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
    println!("  check-kexec-tarball <tarball>  Verify nixos-anywhere kexec tarball shape");
}

fn check_kexec_tarball(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("tarball does not exist: {}", path.display()));
    }

    let output = Command::new("tar")
        .arg("-tf")
        .arg(path)
        .output()
        .map_err(|error| format!("failed to execute tar: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "tar failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let entries = String::from_utf8(output.stdout)
        .map_err(|error| format!("tar output was not UTF-8: {error}"))?;
    let required = ["kexec/run", "kexec/bzImage", "kexec/initrd.gz"];
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|required_entry| !entries.lines().any(|entry| entry == *required_entry))
        .collect();

    if !missing.is_empty() {
        return Err(format!("missing required entries: {}", missing.join(", ")));
    }

    println!("ok: {} contains nixos-anywhere kexec layout", path.display());
    Ok(())
}
