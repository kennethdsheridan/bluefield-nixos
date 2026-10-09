# Baseline installed-DPU profile. Keep this module recovery-first: the defaults
# preserve RShim/tmfifo access and an AArch64 removable EFI fallback before
# enabling conveniences such as promoted EFI boot entries.
{ config, lib, modulesPath, pkgs, ... }:

let
  cfg = config.bluefield;
  credentials = cfg.credentials;
in
{
  imports = [
    (modulesPath + "/installer/scan/not-detected.nix")
    ./bluefield-control-plane.nix
    ./bluefield-credentials.nix
    ./bluefield-network.nix
  ];

  options.bluefield = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Enable the baseline NVIDIA BlueField DPU NixOS profile.";
    };

    boot = {
      removableEfi = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Maintain the AArch64 removable EFI fallback path.";
      };

      promoteEfiBootEntry = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Allow GRUB to update EFI variables for the NixOS boot entry.";
      };

      copyKernels = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Copy kernels and initrds under /boot for GRUB.";
      };
    };

    initrdSmoke.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Emit an initrd tmfifo smoke signal during early boot.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !credentials.requireKeys
          || credentials.rootAuthorizedKeys != [ ]
          || credentials.adminUser == "root"
          || credentials.passwordlessSudo;
        message = ''
          BlueField DPU key-only admin access needs a privileged recovery path.
          Set bluefield.credentials.rootAuthorizedKeys, use adminUser = "root",
          or explicitly enable bluefield.credentials.passwordlessSudo.
        '';
      }
    ];

    nixpkgs.hostPlatform = lib.mkDefault "aarch64-linux";

    boot = {
      kernelParams = [
        "console=hvc0"
        "console=ttyAMA0,115200"
        "earlycon=pl011,0x01000000"
        "fixrtc"
        "biosdevname=0"
        "iommu.passthrough=1"
      ];

      initrd.availableKernelModules = [
        "dw_mmc-bluefield"
        "mmc_block"
        "mlxbf-tmfifo"
        "sd_mod"
        "virtio_net"
        "virtio_pci"
      ];

      initrd.kernelModules = [
        "mlxbf-tmfifo"
        "virtio_net"
      ];

      kernelModules = [
        "mlx5_core"
        "mlxbf_gige"
        "mlxbf-tmfifo"
        "virtio_net"
      ];

      extraModulePackages = [ ];

      initrd.systemd.services.bluefield-tmfifo-smoke = lib.mkIf cfg.initrdSmoke.enable {
        description = "Emit a BlueField tmfifo smoke signal before mounting root";
        wantedBy = [ "initrd-root-device.target" ];
        before = [ "initrd-root-device.target" ];
        after = [ "systemd-modules-load.service" "systemd-udevd.service" ];
        unitConfig.DefaultDependencies = false;
        serviceConfig.Type = "oneshot";
        script = ''
          log() { printf '<6>bluefield-tmfifo-smoke: %s\n' "$*" > /dev/kmsg; }

          log start
          modprobe mlxbf-tmfifo || true
          modprobe virtio_net || true

          # The tmfifo device can appear under platform virtio paths before it
          # has the final stable interface name. Poll both locations so early
          # boot evidence still works across kernel enumeration timing changes.
          for _ in $(seq 1 60); do
            tmfifo_if=""
            for net_path in /sys/devices/platform/MLNXBF01:00/virtio*/net/*; do
              if [ -e "$net_path" ]; then
                tmfifo_if="''${net_path##*/}"
                break
              fi
            done
            if [ -z "$tmfifo_if" ] && [ -e /sys/class/net/${cfg.tmfifo.interfaceName} ]; then
              tmfifo_if="${cfg.tmfifo.interfaceName}"
            fi

            if [ -n "$tmfifo_if" ]; then
              log "tmfifo interface present as $tmfifo_if"
              ip link set "$tmfifo_if" name ${cfg.tmfifo.interfaceName} 2>/dev/null || true
              [ -e /sys/class/net/${cfg.tmfifo.interfaceName} ] && tmfifo_if="${cfg.tmfifo.interfaceName}"
              ip link set "$tmfifo_if" up || true
              ip address add ${cfg.tmfifo.address} dev "$tmfifo_if" 2>/dev/null || true
              ping -c 1 -W 1 ${cfg.tmfifo.peerAddress} || true
              break
            fi

            log "waiting for ${cfg.tmfifo.interfaceName}"
            sleep 1
          done
          log done
        '';
      };

      initrd.systemd.initrdBin = [
        pkgs.coreutils
        pkgs.iproute2
        pkgs.iputils
        pkgs.kmod
      ];

      loader = {
        efi = {
          canTouchEfiVariables = cfg.boot.promoteEfiBootEntry;
          efiSysMountPoint = "/boot/efi";
        };
        grub = {
          enable = true;
          efiSupport = true;
          efiInstallAsRemovable = cfg.boot.removableEfi && !cfg.boot.promoteEfiBootEntry;
          copyKernels = cfg.boot.copyKernels;
          device = "";
          mirroredBoots = [
            {
              path = "/boot";
              devices = [ "nodev" ];
              efiSysMountPoint = "/boot/efi";
              efiBootloaderId = "NixOS";
            }
          ];
          # When GRUB is allowed to write a named EFI boot entry, still refresh
          # the removable fallback path used by BlueField UEFI recovery flows.
          extraInstallCommands = lib.mkIf (cfg.boot.removableEfi && cfg.boot.promoteEfiBootEntry) ''
            grub_efi=/boot/efi/EFI/NixOS/grubaa64.efi
            fallback_efi=/boot/efi/EFI/BOOT/BOOTAA64.EFI

            if [ -e "$grub_efi" ]; then
              ${pkgs.coreutils}/bin/install -D -m 0644 "$grub_efi" "$fallback_efi"
            elif [ ! -e "$fallback_efi" ]; then
              echo "missing $grub_efi and $fallback_efi; cannot preserve BlueField removable EFI fallback" >&2
              exit 1
            fi
          '';
        };
      };
    };

    users.users = {
      root.openssh.authorizedKeys.keys = credentials.rootAuthorizedKeys
        ++ lib.optionals (credentials.adminUser == "root") credentials.authorizedKeys;
    } // lib.optionalAttrs (credentials.adminUser != "root") {
      ${credentials.adminUser} = {
        isNormalUser = true;
        description = credentials.adminDescription;
        shell = pkgs.bashInteractive;
        extraGroups = [ "wheel" ];
        openssh.authorizedKeys.keys = credentials.authorizedKeys;
      };
    };

    nix.settings.trusted-users = [ "root" ] ++ credentials.trustedUsers;
    security.sudo.wheelNeedsPassword = lib.mkDefault (!credentials.passwordlessSudo);

    services.openssh = {
      enable = true;
      settings = {
        PasswordAuthentication = false;
        PermitRootLogin = "prohibit-password";
      };
    };

    environment.systemPackages = with pkgs; [
      curl
      ethtool
      efibootmgr
      gitMinimal
      iperf3
      iproute2
      lldpd
      neovim
      pciutils
      rdma-core
      tcpdump
      vim
    ];

    programs.neovim = {
      enable = true;
      defaultEditor = true;
    };

    environment.variables = {
      EDITOR = "nvim";
      VISUAL = "nvim";
    };
  };
}
