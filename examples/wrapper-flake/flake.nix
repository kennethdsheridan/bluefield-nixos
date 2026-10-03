{
  description = "Private BlueField DPU wrapper flake";

  inputs = {
    bluefield-nixos.url = "github:OWNER/bluefield-nixos";
    nixpkgs.follows = "bluefield-nixos/nixpkgs";
  };

  outputs = { bluefield-nixos, nixpkgs, ... }: {
    nixosConfigurations.my-bluefield = nixpkgs.lib.nixosSystem {
      system = "aarch64-linux";
      modules = [
        bluefield-nixos.nixosModules.bluefield-dpu
        {
          system.stateVersion = "25.11";
          networking.hostName = "my-bluefield";

          bluefield.credentials = {
            requireKeys = true;
            adminUser = "admin";
            authorizedKeys = [ "ssh-ed25519 <operator-public-key>" ];
            rootAuthorizedKeys = [ "ssh-ed25519 <operator-public-key>" ];
            trustedUsers = [ "admin" ];
            passwordlessSudo = false;
          };
        }
      ];
    };
  };
}
