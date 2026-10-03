use std::env;
use std::fs::File;
use std::io::BufReader;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

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
