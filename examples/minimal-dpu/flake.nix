{
  description = "Minimal BlueField DPU host example";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    bluefield-nixos.url = "github:OWNER/bluefield-nixos";
  };

  outputs = { nixpkgs, bluefield-nixos, ... }: {
    nixosConfigurations.bluefield-dpu = nixpkgs.lib.nixosSystem {
      system = "aarch64-linux";
      modules = [
        bluefield-nixos.nixosModules.bluefield-dpu
        ./configuration.nix
      ];
    };
  };
}
