# Credential policy shared by the installed DPU and kexec installer profiles.
# Public modules do not embed keys; private wrapper flakes provide them through
# these options so the reusable repo can stay safe to publish.
{ config, lib, ... }:

let
  cfg = config.bluefield.credentials;
  hasAnyKey = cfg.authorizedKeys != [ ] || cfg.rootAuthorizedKeys != [ ];
in
{
  options.bluefield.credentials = {
    adminUser = lib.mkOption {
      type = lib.types.str;
      default = "bluefield-admin";
      description = "Local administrator user to provision on the BlueField DPU.";
    };

    adminDescription = lib.mkOption {
      type = lib.types.str;
      default = "BlueField administrator";
      description = "GECOS description for the local BlueField administrator user.";
    };

    authorizedKeys = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "SSH public keys authorized for the BlueField administrator user.";
    };

    rootAuthorizedKeys = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "SSH public keys authorized for root on the BlueField DPU.";
    };

    trustedUsers = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "Extra Nix trusted-users entries for the BlueField DPU.";
    };

    passwordlessSudo = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Allow the configured administrator user to use sudo without a password.
        This is an explicit break-glass option for key-only installs.
      '';
    };

    requireKeys = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Fail evaluation when neither administrator nor root SSH public keys are
        configured. Enable this for installable images and kexec installers so a
        build cannot accidentally produce an unreachable DPU.
      '';
    };
  };

  config.assertions = [
    {
      assertion = !cfg.requireKeys || hasAnyKey;
      message = ''
        BlueField DPU SSH keys are required for this build. Set
        bluefield.credentials.authorizedKeys or
        bluefield.credentials.rootAuthorizedKeys from a private wrapper flake or
        deployment-local module.
      '';
    }
  ];

  config.warnings = lib.optional (!hasAnyKey) ''
    BlueField DPU has no SSH authorized keys configured. Add a deployment-local
    module that sets bluefield.credentials.authorizedKeys or
    bluefield.credentials.rootAuthorizedKeys before building an installable image.
  '';
}
