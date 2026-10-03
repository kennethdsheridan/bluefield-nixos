# Install workflow

This workflow uses `nixos-anywhere` to boot a temporary installer on the DPU,
then optionally rewrites the BlueField eMMC user area. Treat every destructive
step as hardware-specific until you have verified console and BFB recovery.

## Prerequisites

- A BlueField DPU with working RShim console access.
- A known-good vendor BFB recovery image for the target board and PSID.
- Host-side tmfifo networking configured for the DPU peer address.
- `nixos-anywhere`, `disko`, and Nix flakes available on the operator machine.
- Operator SSH keys supplied from a private wrapper flake, not this repository.

The modules default to the common RShim tmfifo link:

- Host side: `192.168.100.1/30`
- DPU side: `192.168.100.2/30`
- DPU interface: `tmfifo_net0`

Override `bluefield.tmfifo.*` if your environment differs.

## Build the private installer

Start from `examples/wrapper-flake`, replace `operatorKey`, and point the input
at your public or local `bluefield-nixos` checkout.

```bash
nix build .#packages.aarch64-linux.bluefield-kexec-anywhere
```

Validate the generated tarball before using it:

```bash
nix run github:OWNER/bluefield-nixos#bluefield-validate -- \
  check-kexec-tarball ./result/tarball/*.tar.xz
```

`check-kexec-tarball` validates archive metadata and layout only. Use it only on
tarballs you built locally or otherwise trust.

## Smoke-test kexec

Run the kexec phase first. A temporary SSH disconnect is expected while the DPU
switches into the installer.

```bash
nixos-anywhere \
  --phases kexec \
  --kexec ./result/tarball/*.tar.xz \
  --target-host root@192.168.100.2
```

After the DPU reboots into the installer, verify access before installing:

```bash
ssh root@192.168.100.2 hostname
ssh root@192.168.100.2 systemctl is-active sshd
ssh root@192.168.100.2 ip address show tmfifo_net0
```

The expected installer hostname is `bluefield-dpu-installer` unless overridden.

## Install to eMMC

The included disko example erases `/dev/mmcblk0` and intentionally leaves
`/dev/mmcblk0boot0` and `/dev/mmcblk0boot1` unmanaged. Import it only from a
private host configuration after confirming the target device.

```nix
{
  imports = [
    bluefield-nixos.nixosModules.bluefield-dpu
    "${bluefield-nixos}/examples/disko-emmc/disk-config.nix"
  ];

  bluefield.install = {
    confirmDestructiveEmmc = true;
    emmcDevice = "/dev/mmcblk0";
  };
}
```

Then run the full install from the private wrapper:

```bash
nixos-anywhere \
  --flake .#my-bluefield \
  --kexec ./result/tarball/*.tar.xz \
  --target-host root@192.168.100.2
```

If the completed install drops to the UEFI shell, try the removable fallback
before reflashing:

```text
FS0:\EFI\BOOT\BOOTAA64.EFI
```

If that boots NixOS, recreate the stale EFI entry with `efibootmgr` or leave the
removable fallback enabled.

## Build a BFB installer

If you have a compatible NVIDIA carrier BFB, you can wrap the same kexec tarball
as a BFB for RShim boot delivery:

```bash
nix run .#bluefield-build-bfb -- \
  --base-bfb /path/to/bf-bundle-<release>.bfb \
  --kexec-tarball ./result/tarball/bluefield-kexec-anywhere-<label>.tar.xz \
  --output /tmp/nixos-bluefield-installer.bfb
```

Inspect and boot only after confirming the target device and recovery path:

```bash
nix shell nixpkgs#bfscripts -c mlx-mkbfb -d /tmp/nixos-bluefield-installer.bfb
sudo bfb-install --rshim rshim0 --bfb /tmp/nixos-bluefield-installer.bfb
```
