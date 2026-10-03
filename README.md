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
- `templates.minimal-dpu`
- `templates.wrapper-flake`

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
