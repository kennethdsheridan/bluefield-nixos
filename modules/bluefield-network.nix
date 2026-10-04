# Shared BlueField networking defaults for both installed systems and installer
# images. tmfifo is kept deterministic because it is the primary RShim recovery
# path when normal Ethernet or OOB management is not available.
{ config, lib, pkgs, ... }:

let
  cfg = config.bluefield;
in
{
  options.bluefield = {
    tmfifo = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable RShim tmfifo management networking.";
      };

      interfaceName = lib.mkOption {
        type = lib.types.str;
        default = "tmfifo_net0";
        description = "Stable interface name for the RShim tmfifo network device.";
      };

      address = lib.mkOption {
        type = lib.types.str;
        default = "192.168.100.2/30";
        description = "CIDR address assigned to the DPU side of the tmfifo link.";
      };

      peerAddress = lib.mkOption {
        type = lib.types.str;
        default = "192.168.100.1";
        description = "Host-side tmfifo peer address.";
      };

      defaultRoute = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Use the host-side tmfifo peer as the default gateway.";
      };
    };

    oob = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable the BlueField out-of-band management interface.";
      };

      interfaceName = lib.mkOption {
        type = lib.types.str;
        default = "oob_net0";
        description = "Out-of-band management interface name.";
      };

      dhcp = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Use DHCP on the out-of-band management interface.";
      };
    };
  };

  config = lib.mkMerge [
    (lib.mkIf (cfg.tmfifo.enable || cfg.oob.enable) {
      networking.useDHCP = false;
      networking.useNetworkd = true;
    })

    (lib.mkIf cfg.tmfifo.enable {
      networking.firewall.interfaces.${cfg.tmfifo.interfaceName}.allowedTCPPorts = [ 22 ];

      systemd.network.networks."10-bluefield-tmfifo" = {
        matchConfig.Name = cfg.tmfifo.interfaceName;
        networkConfig = {
          Address = cfg.tmfifo.address;
          IPv6AcceptRA = false;
        } // lib.optionalAttrs cfg.tmfifo.defaultRoute {
          Gateway = cfg.tmfifo.peerAddress;
        };
      };

      systemd.services.bluefield-tmfifo-network = {
        description = "Force BlueField tmfifo management networking online";
        wantedBy = [ "multi-user.target" ];
        before = [ "sshd.service" ];
        after = [ "systemd-udevd.service" "systemd-networkd.service" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        path = [ pkgs.coreutils pkgs.iproute2 pkgs.kmod ];
        script = ''
          modprobe mlxbf-tmfifo || true
          modprobe virtio_net || true

          # BlueField kernels may expose the tmfifo netdev before udev has named
          # it. Rename it to the configured stable name before SSH starts.
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
              ip link set "$tmfifo_if" name ${cfg.tmfifo.interfaceName} 2>/dev/null || true
              [ -e /sys/class/net/${cfg.tmfifo.interfaceName} ] && tmfifo_if="${cfg.tmfifo.interfaceName}"
              ip link set "$tmfifo_if" up || true
              ip address replace ${cfg.tmfifo.address} dev "$tmfifo_if"
              exit 0
            fi

            sleep 1
          done

          exit 1
        '';
      };
    })

    (lib.mkIf (cfg.oob.enable && cfg.oob.dhcp) {
      networking.firewall.interfaces.${cfg.oob.interfaceName}.allowedTCPPorts = [ 22 ];

      systemd.network.networks."20-bluefield-oob" = {
        matchConfig.Name = cfg.oob.interfaceName;
        networkConfig.DHCP = "ipv4";
      };
    })
  ];
}
