# bluefield-nixos

`bluefield-nixos` is a reference flake for running NixOS on NVIDIA BlueField
DPUs. It starts with recovery-first modules and examples extracted from validated
BlueField-2 lab work.

## Status

This project is alpha-quality. Use it only on recoverable hardware with working
RShim console access and a known BFB recovery path.

Validated lab result:

- A custom BlueField kexec installer returned SSH over `tmfifo_net0`.
- A destructive `/dev/mmcblk0` install completed on BlueField-2 hardware.
- Full-disk `disko` recreated the ESP PARTUUID.
- A stale EFI entry dropped the DPU to UEFI shell until `FS0:\EFI\BOOT\BOOTAA64.EFI`
  was selected and the `NixOS` entry was repaired with `efibootmgr`.

## Safety Rules

- Do not apply the eMMC disko example until you have console and BFB recovery.
- Keep `/dev/mmcblk0boot0` and `/dev/mmcblk0boot1` unmanaged.
- Keep operator SSH public keys in a private wrapper flake, not this repo.
- Use the removable EFI fallback before reflashing if an install reaches UEFI
  shell after `nixos-anywhere` completes.

## Outputs

- `nixosModules.bluefield-credentials`
- `nixosModules.bluefield-network`
- `nixosModules.bluefield-dpu`
- `nixosModules.bluefield-kexec-installer`
- `packages.<system>.bluefield-validate`
- `packages.<system>.bluefield-build-bfb`
- `apps.<system>.bluefield-build-bfb`
- `templates.minimal-dpu`
- `templates.wrapper-flake`

## Documentation

- [Install workflow](docs/install.md): build a private kexec installer, smoke-test
  tmfifo SSH, install to eMMC, and optionally wrap the installer as a BFB.
- [Credentials](docs/credentials.md): keep operator SSH keys in private flakes.
- [Recovery](docs/recovery.md): recover from stale EFI entries after eMMC writes.

## Quick Checks

```bash
nix flake show
nix flake check
nix build .#bluefield-validate
```

`bluefield-validate check-kexec-tarball` is a metadata safety check, not an
authenticity check. Run it only on locally built or otherwise trusted tarballs.
It requires nixos-anywhere kexec tarballs to contain only safe relative
regular-file and directory entries, owned by `0:0`, without writable group/world
bits or special mode bits.

## Build a BFB Installer

`bluefield-build-bfb` repacks a trusted nixos-anywhere kexec tarball into a
BlueField bootstream by calling NVIDIA's `mlx-mkbfb` from `nixpkgs#bfscripts`.
It requires a compatible NVIDIA BFB as the carrier image; keep that file outside
the repository.

```bash
nix run .#bluefield-build-bfb -- \
  --base-bfb ~/Downloads/bf-bundle-<release>.bfb \
  --kexec-tarball ./result/tarball/bluefield-kexec-anywhere-<label>.tar.xz \
  --output /tmp/nixos-bluefield-installer.bfb
```

The command validates the kexec tarball metadata, extracts only the kernel,
initrd, and run script, derives the kernel command line, writes the BFB, and runs
`mlx-mkbfb -c` on the result. Inspect the result before booting it:

```bash
nix shell nixpkgs#bfscripts -c mlx-mkbfb -d /tmp/nixos-bluefield-installer.bfb
```

Boot through RShim only after the target device, PSID, and recovery path are
confirmed:

```bash
sudo bfb-install --rshim rshim0 --bfb /tmp/nixos-bluefield-installer.bfb
```

Prefer a boot-only carrier BFB when available. A full BF-Bundle carrier may
include destructive firmware or OS-install payloads if used incorrectly.

## Credential Model

Bootstrap SSH public keys identify an operator. Keep them out of the public repo
and pass them from a private wrapper flake:

```nix
bluefield.credentials = {
  requireKeys = true;
  adminUser = "admin";
  authorizedKeys = [ "ssh-ed25519 <operator-public-key>" ];
  rootAuthorizedKeys = [ "ssh-ed25519 <operator-public-key>" ];
  trustedUsers = [ "admin" ];
};
```

Use agenix, sops-nix, or SecretSpec later for runtime secrets after the installed
system exists.
