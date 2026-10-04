# Ephemeral nixos-anywhere installer profile for BlueField DPUs. It intentionally
# favors SSH/tmfifo reachability over host-specific persistence so a failed full
# install can be retried from the management link.
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

  options.bluefield.kexecInstaller = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Enable the BlueField nixos-anywhere kexec installer profile.";
    };

    enableNixosBootstrapUser = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Also authorize the standard installer `nixos` user. By default only root
        and the configured BlueField administrator receive SSH keys.
      '';
    };
  };

  config = lib.mkIf cfg.kexecInstaller.enable {
    system.stateVersion = lib.mkDefault "25.11";
    networking.hostName = lib.mkDefault "bluefield-dpu-installer";
    nixpkgs.hostPlatform = lib.mkDefault "aarch64-linux";
    bluefield.credentials.requireKeys = lib.mkDefault true;

    assertions = [
      {
        assertion = pkgs.stdenv.hostPlatform.system == "aarch64-linux";
        message = "The BlueField kexec installer must target aarch64-linux.";
      }
    ];

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
      # Root always receives recovery keys. The normal installer `nixos` user is
      # optional so public examples do not imply an extra default login surface.
      root.openssh.authorizedKeys.keys = credentials.rootAuthorizedKeys
        ++ lib.optionals (credentials.adminUser == "root") credentials.authorizedKeys;
    } // lib.optionalAttrs (cfg.kexecInstaller.enableNixosBootstrapUser || credentials.adminUser == "nixos") {
      nixos.openssh.authorizedKeys.keys = credentials.authorizedKeys;
    } // lib.optionalAttrs (credentials.adminUser != "root" && credentials.adminUser != "nixos") {
      ${credentials.adminUser} = {
        isNormalUser = true;
        description = credentials.adminDescription;
        shell = pkgs.bashInteractive;
        extraGroups = [ "wheel" ];
        openssh.authorizedKeys.keys = credentials.authorizedKeys;
      };
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
