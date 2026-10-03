# Destructive BlueField eMMC layout for nixos-anywhere.
#
# WARNING: This erases /dev/mmcblk0. It intentionally leaves the eMMC boot
# devices unmanaged:
# - /dev/mmcblk0boot0
# - /dev/mmcblk0boot1
#
# Applying this layout recreates partition GUIDs. Validate RShim console access,
# BFB recovery, and EFI boot-entry behavior before using it.
{ ... }:

{
  disko.devices = {
    disk = {
      emmc = {
        type = "disk";
        device = "/dev/mmcblk0";
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
