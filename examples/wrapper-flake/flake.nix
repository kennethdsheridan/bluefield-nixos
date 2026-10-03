{
  description = "Private BlueField DPU wrapper flake";

  inputs = {
    bluefield-nixos.url = "github:OWNER/bluefield-nixos";
    nixpkgs.follows = "bluefield-nixos/nixpkgs";
  };

  outputs = { bluefield-nixos, nixpkgs, ... }:
    let
      system = "aarch64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      operatorKey = "ssh-ed25519 <operator-public-key>";
      credentialModule = {
        bluefield.credentials = {
          requireKeys = true;
          adminUser = "admin";
          authorizedKeys = [ operatorKey ];
          rootAuthorizedKeys = [ operatorKey ];
          trustedUsers = [ "admin" ];
          passwordlessSudo = false;
        };
      };
      dpu = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [
          bluefield-nixos.nixosModules.bluefield-dpu
          credentialModule
          {
            system.stateVersion = "25.11";
            networking.hostName = "my-bluefield";
          }
        ];
      };
      kexecInstaller = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [
          bluefield-nixos.nixosModules.bluefield-kexec-installer
          credentialModule
        ];
      };
    in
    {
      nixosConfigurations.my-bluefield = dpu;
      nixosConfigurations.my-bluefield-kexec-installer = kexecInstaller;

      packages.${system}.bluefield-kexec-anywhere =
        bluefield-nixos.lib.mkBluefieldKexecAnywhereTarball {
          inherit pkgs;
          kexecConfig = kexecInstaller.config;
          name = "my-bluefield-kexec-anywhere";
        };
    };
}
