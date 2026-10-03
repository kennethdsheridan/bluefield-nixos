{
  description = "NixOS modules and installer helpers for NVIDIA BlueField DPUs";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      nixosModules = {
        bluefield-credentials = ./modules/bluefield-credentials.nix;
        bluefield-network = ./modules/bluefield-network.nix;
        bluefield-dpu = ./modules/bluefield-dpu.nix;
        bluefield-kexec-installer = ./modules/bluefield-kexec-installer.nix;
        default = ./modules/bluefield-dpu.nix;
      };

      packages = forAllSystems (pkgs: {
        bluefield-validate = pkgs.callPackage ./tools/bluefield-validate/package.nix { };
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.bluefield-validate;
      });

      checks = forAllSystems (pkgs: {
        bluefield-validate = self.packages.${pkgs.stdenv.hostPlatform.system}.bluefield-validate;
      });

      formatter = forAllSystems (pkgs: pkgs.nixpkgs-fmt);

      templates = {
        minimal-dpu = {
          path = ./examples/minimal-dpu;
          description = "Minimal NixOS host for NVIDIA BlueField DPU";
        };
        wrapper-flake = {
          path = ./examples/wrapper-flake;
          description = "Private wrapper flake for BlueField operator SSH keys";
        };
      };
    };
}
