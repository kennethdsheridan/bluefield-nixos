# Declarative BlueField control-plane intent. This module is intentionally safe by
# default: it records desired routing, EVPN, BGP, VPC, and DOCA state and runs a
# reconciliation timer in plan/report mode unless apply is explicitly enabled.
{ config, lib, pkgs, ... }:

let
  cfg = config.bluefield.controlPlane;

  jsonFormat = pkgs.formats.json { };

  routeType = lib.types.submodule {
    options = {
      destination = lib.mkOption {
        type = lib.types.str;
        description = "Route destination prefix, for example 10.42.0.0/16.";
      };

      gateway = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Optional next-hop address.";
      };

      interface = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Optional egress interface.";
      };

      table = lib.mkOption {
        type = lib.types.nullOr lib.types.int;
        default = null;
        description = "Optional Linux routing table.";
      };
    };
  };

  bgpNeighborType = lib.types.submodule {
    options = {
      address = lib.mkOption {
        type = lib.types.str;
        description = "BGP peer address.";
      };

      remoteAs = lib.mkOption {
        type = lib.types.int;
        description = "Remote autonomous system number.";
      };

      description = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Optional peer description.";
      };
    };
  };

  vpcType = lib.types.submodule {
    options = {
      vni = lib.mkOption {
        type = lib.types.int;
        description = "VXLAN network identifier for this VPC.";
      };

      cidrs = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = "CIDR blocks owned by this VPC.";
      };

      bridge = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Optional Linux bridge associated with this VPC.";
      };
    };
  };

  docaServiceType = lib.types.submodule {
    options = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Whether this DOCA service should be reconciled.";
      };

      package = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Optional package or image reference for the DOCA component.";
      };

      settings = lib.mkOption {
        type = jsonFormat.type;
        default = { };
        description = "Service-specific declarative settings.";
      };
    };
  };

  intent = {
    node = cfg.nodeName;
    apply = cfg.apply;
    routing = {
      inherit (cfg.routing) enable routes;
    };
    bgp = {
      inherit (cfg.bgp) enable localAs routerId neighbors networks;
    };
    evpn = {
      inherit (cfg.evpn) enable vnis advertiseAllVni;
    };
    vpcs = cfg.vpcs;
    doca = {
      inherit (cfg.doca) enable services;
    };
  };

  intentFile = jsonFormat.generate "bluefield-control-plane.json" intent;
in
{
  options.bluefield.controlPlane = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Enable the BlueField declarative control-plane reconciler.";
    };

    apply = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Apply reconciled state instead of reporting the desired intent only.";
    };

    nodeName = lib.mkOption {
      type = lib.types.str;
      default = config.networking.hostName or "bluefield-dpu";
      description = "Stable node name used in generated control-plane intent.";
    };

    interval = lib.mkOption {
      type = lib.types.str;
      default = "5min";
      description = "systemd timer interval for reconciliation.";
    };

    routing = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Enable declarative route reconciliation.";
      };

      routes = lib.mkOption {
        type = lib.types.listOf routeType;
        default = [ ];
        description = "Desired Linux routes managed by the DPU reconciler.";
      };
    };

    bgp = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Enable declarative BGP intent.";
      };

      localAs = lib.mkOption {
        type = lib.types.nullOr lib.types.int;
        default = null;
        description = "Local autonomous system number.";
      };

      routerId = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "BGP router ID.";
      };

      neighbors = lib.mkOption {
        type = lib.types.listOf bgpNeighborType;
        default = [ ];
        description = "Desired BGP neighbors.";
      };

      networks = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = "Prefixes to advertise through BGP.";
      };
    };

    evpn = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Enable EVPN intent for VPC overlays.";
      };

      vnis = lib.mkOption {
        type = lib.types.listOf lib.types.int;
        default = [ ];
        description = "VXLAN VNIs expected on this DPU.";
      };

      advertiseAllVni = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Advertise all configured VNIs when EVPN is enabled.";
      };
    };

    vpcs = lib.mkOption {
      type = lib.types.attrsOf vpcType;
      default = { };
      description = "Desired VPC overlays keyed by stable VPC name.";
    };

    doca = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Enable declarative DOCA service intent.";
      };

      services = lib.mkOption {
        type = lib.types.attrsOf docaServiceType;
        default = { };
        description = "Desired DOCA services keyed by component name.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !cfg.bgp.enable || cfg.bgp.localAs != null;
        message = "bluefield.controlPlane.bgp.localAs is required when BGP intent is enabled.";
      }
      {
        assertion = !cfg.evpn.enable || cfg.bgp.enable;
        message = "BlueField EVPN intent requires BGP intent to be enabled.";
      }
      {
        assertion = lib.all (route: route.gateway != null || route.interface != null) cfg.routing.routes;
        message = "Each BlueField control-plane route needs either a gateway or an interface.";
      }
    ];

    warnings = lib.optional (!cfg.apply) ''
      BlueField control-plane reconciliation is enabled in report-only mode.
      Set bluefield.controlPlane.apply = true after validating generated intent.
    '';

    environment.etc."bluefield/control-plane.json".source = intentFile;

    systemd.services.bluefield-control-plane-reconcile = {
      description = "Reconcile declarative BlueField control-plane intent";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      serviceConfig = {
        Type = "oneshot";
        DynamicUser = true;
        StateDirectory = "bluefield-control-plane";
      };
      path = [ pkgs.coreutils pkgs.jq ];
      script = ''
        set -eu
        intent=/etc/bluefield/control-plane.json
        state=/var/lib/bluefield-control-plane/last-intent.json

        jq empty "$intent"
        cp "$intent" "$state"

        if [ "$(jq -r '.apply' "$intent")" != "true" ]; then
          jq -c '{node, routing, bgp, evpn, vpcs, doca}' "$intent"
          exit 0
        fi

        echo "bluefield-control-plane: apply mode is not implemented yet; keep apply=false until protocol-specific reconcilers are wired" >&2
        exit 1
      '';
    };

    systemd.timers.bluefield-control-plane-reconcile = {
      description = "Periodic BlueField control-plane reconciliation";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "2min";
        OnUnitActiveSec = cfg.interval;
        Unit = "bluefield-control-plane-reconcile.service";
      };
    };
  };
}
