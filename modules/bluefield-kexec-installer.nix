{ config, lib, modulesPath, pkgs, ... }:

let
  cfg = config.bluefield;
  credentials = cfg.credentials;
in
{
  imports = [
    (modulesPath + "/installer/netboot/netboot-minimal.nix")
    ./bluefield-credentials.nix
    ./bluefield-network.nix
  ];

  options.bluefield.kexecInstaller.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Enable the BlueField nixos-anywhere kexec installer profile.";
  };

  config = lib.mkIf cfg.kexecInstaller.enable {
    system.stateVersion = lib.mkDefault "25.11";
    networking.hostName = lib.mkDefault "bluefield-dpu-installer";

    boot = {
      zfs.forceImportRoot = false;
      initrd.includeDefaultModules = false;

      kernelParams = [
        "console=hvc0"
        "console=ttyAMA0,115200"
        "earlycon=pl011,0x01000000"
        "fixrtc"
        "biosdevname=0"
        "iommu.passthrough=1"
      ];

      initrd.availableKernelModules = lib.mkForce [
        "dw_mmc-bluefield"
        "mmc_block"
        "mlxbf-tmfifo"
        "sd_mod"
        "squashfs"
        "overlay"
        "virtio_net"
        "virtio_pci"
      ];

      initrd.kernelModules = lib.mkForce [
        "loop"
        "mlxbf-tmfifo"
        "overlay"
        "virtio_net"
      ];

      kernelModules = [
        "mlx5_core"
        "mlxbf_gige"
        "mlxbf-tmfifo"
        "virtio_net"
      ];

      initrd.systemd.initrdBin = [
        pkgs.coreutils
        pkgs.iproute2
        pkgs.iputils
        pkgs.kmod
      ];
    };

    services.openssh = {
      enable = true;
      settings = {
        PasswordAuthentication = false;
        PermitRootLogin = "prohibit-password";
      };
    };

    users.users = {
      root.openssh.authorizedKeys.keys = credentials.rootAuthorizedKeys;
      nixos.openssh.authorizedKeys.keys = credentials.authorizedKeys;
    };

    environment.systemPackages = with pkgs; [
      ethtool
      iproute2
      kexec-tools
      pciutils
      tcpdump
    ];
  };
}
