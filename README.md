# bluefield-nixos

`bluefield-nixos` is a recovery-first Nix flake for booting and installing
NixOS on NVIDIA BlueField DPUs.

## Status

Alpha. Use it only on recoverable hardware with working RShim console access and
a known-good NVIDIA BFB recovery path.

Validated lab result:

- BlueField-2 booted a custom NixOS kexec installer over RShim `tmfifo_net0`.
- A destructive `/dev/mmcblk0` `nixos-anywhere` install completed.
- A stale UEFI boot entry was recovered through `FS0:\EFI\BOOT\BOOTAA64.EFI`.

## Start Here

- [Install workflow](docs/install.md): build a private kexec installer, smoke-test
  SSH over tmfifo, install to eMMC, and optionally wrap the installer as a BFB.
- [Recovery](docs/recovery.md): recover from stale UEFI boot entries and avoid
  unsafe eMMC boot-partition writes.
- [Credentials](docs/credentials.md): keep operator SSH keys in private flakes.

## Outputs

- `nixosModules.bluefield-credentials`
- `nixosModules.bluefield-control-plane`
- `nixosModules.bluefield-network`
- `nixosModules.bluefield-dpu`
- `nixosModules.bluefield-kexec-installer`
- `packages.<system>.bluefield-validate`
- `packages.<system>.bluefield-build-bfb`
- `packages.<system>.bluefield-install-bfb`
- `packages.<system>.bluefield-repair-host-tmfifo`
- `apps.<system>.bluefield-build-bfb`
- `apps.<system>.bluefield-install-bfb`
- `apps.<system>.bluefield-repair-host-tmfifo`
- `templates.minimal-dpu`
- `templates.wrapper-flake`

## Quick Checks

```bash
nix flake show
nix flake check
nix build .#bluefield-validate
```

## Safety Rules

- Do not apply the eMMC `disko` example until RShim console and BFB recovery are
  verified.
- Keep `/dev/mmcblk0boot0` and `/dev/mmcblk0boot1` unmanaged unless you are
  intentionally following NVIDIA boot-partition update procedures.
- Keep SSH public keys and operator-specific host config outside this public repo.
- Use the removable EFI fallback before reflashing if an install reaches the UEFI
  shell.
- Stop BFB streaming when RShim does not report a populated `OPN_STR`; follow
  the recovery runbook and use a platform reset instead of repeating
  `bfb-install` attempts.

## NVIDIA References

- [NVIDIA BlueField BSP](https://networking-docs.nvidia.com/bsp): vendor BSP,
  boot, management, and recovery documentation.
- [SoC Management Interface (RShim)](https://networking-docs.nvidia.com/bsp/4.16.0/soc-management-interface-rshim):
  `/dev/rshim<N>`, console, `tmfifo_net<N>`, `bfb-install`, and RShim ownership.
- [BF-Bundle Installation and Upgrade](https://docs.nvidia.com/doca/archive/3-5-0/bf-bundle-installation-and-upgrade):
  NVIDIA BFB deployment, tmfifo defaults, DOCA installer behavior, and custom BFB
  notes.
- [DOCA Installation Guide for Linux](https://docs.nvidia.com/doca/archive/3-5-0/doca-installation-guide-for-linux):
  DOCA host/BF-Bundle installation methods, supported OS notes, and the upstream
  `bfb-build` pointer for custom OS images.
- [BlueField Boot Flow and BFB Format](https://networking-docs.nvidia.com/bsp/4.16.0/appendix-bluefield-boot-flow-and-bfb-format):
  BFB structure, `mlx-mkbfb`, UEFI boot entries, UPVS, and eMMC boot-partition
  update tools such as `bfrec` and `mlxbf-bootctl`.

## BFB Installer

`bluefield-build-bfb` repacks a trusted `nixos-anywhere` kexec tarball into a
BlueField bootstream using NVIDIA's `mlx-mkbfb` from `nixpkgs#bfscripts`.

```bash
nix run .#bluefield-build-bfb -- --help
```

See [Install workflow](docs/install.md#build-a-bfb-installer) for the full flow.
Use `bluefield-install-bfb` instead of raw `bfb-install` so protected RShim
nodes, hung streams, and wedged RShim states are handled consistently.

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
