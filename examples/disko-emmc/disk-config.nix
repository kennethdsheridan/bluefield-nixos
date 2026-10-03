# Destructive BlueField eMMC layout for nixos-anywhere.
#
# WARNING: This erases /dev/mmcblk0. It intentionally leaves the eMMC boot
# devices unmanaged:
# - /dev/mmcblk0boot0
# - /dev/mmcblk0boot1
#
# Applying this layout recreates partition GUIDs. Validate RShim console access,
# BFB recovery, and EFI boot-entry behavior before using it.
{ config, lib, ... }:

{
  options.bluefield.install.confirmDestructiveEmmc = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = ''
      Confirm that the operator intends to erase /dev/mmcblk0 on a recoverable
      BlueField DPU and has validated RShim console and BFB recovery.
    '';
  };

  options.bluefield.install.emmcDevice = lib.mkOption {
    type = lib.types.nullOr lib.types.str;
    default = null;
    example = "/dev/mmcblk0";
    description = ''
      Explicit BlueField eMMC user-area block device to erase. This public
      example intentionally accepts only /dev/mmcblk0 and rejects eMMC boot
      partitions.
    '';
  };

  config.assertions = [
    {
      assertion = config.bluefield.install.confirmDestructiveEmmc && config.bluefield.install.emmcDevice != null;
      message = ''
        Refusing to evaluate the destructive BlueField eMMC disko layout. Set
        bluefield.install.confirmDestructiveEmmc = true and
        bluefield.install.emmcDevice only after validating RShim console access,
        BFB recovery, and the expected target device.
      '';
    }
    {
      assertion = !config.bluefield.install.confirmDestructiveEmmc
        || config.bluefield.install.emmcDevice == "/dev/mmcblk0";
      message = ''
        The public BlueField eMMC disko example only permits /dev/mmcblk0. Do
        not point it at /dev/mmcblk0boot0, /dev/mmcblk0boot1, /dev/sdX, or other
        host disks. Fork the example with an additional hardware preflight if
        your board exposes the user-area eMMC under a different stable path.
      '';
    }
  ];

  disko.devices = {
    disk = {
      emmc = {
        type = "disk";
        device = config.bluefield.install.emmcDevice;
        content = {
          type = "gpt";
          partitions = {
            esp = {
              size = "50M";
              type = "EF00";
              content = {
                type = "filesystem";
                format = "vfat";
                mountpoint = "/boot/efi";
                mountOptions = [
                  "fmask=0077"
                  "dmask=0077"
                ];
              };
            };
            root = {
              size = "100%";
              content = {
                type = "filesystem";
                format = "ext4";
                mountpoint = "/";
              };
            };
          };
        };
      };
    };
  };
}
