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
      lib = {
        mkBluefieldKexecAnywhereTarball = { pkgs, kexecConfig, name ? "bluefield-kexec-anywhere" }:
          pkgs.runCommand "${name}-tarball"
            {
              nativeBuildInputs = with pkgs; [ gnutar xz ];
            } ''
            mkdir -p "$out/tarball" work/kexec

            install -D -m 0755 ${kexecConfig.system.build.kexecTree}/kexec-boot work/kexec/run
            install -D -m 0644 ${kexecConfig.system.build.kexecTree}/bzImage work/kexec/bzImage
            install -D -m 0644 ${kexecConfig.system.build.kexecTree}/initrd.gz work/kexec/initrd.gz

            tar --owner=0 --group=0 --numeric-owner \
              --mode='u+rwX,go+rX,go-w' \
              -C work -cJf "$out/tarball/${name}-${kexecConfig.system.nixos.label}.tar.xz" \
              kexec
          '';
      };

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

      checks = forAllSystems (pkgs:
        let
          dummyKey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakePublicKeyForEvaluationOnly000000000000 bluefield-nixos-check";
          dpuSystem = lib.nixosSystem {
            system = "aarch64-linux";
            modules = [
              self.nixosModules.bluefield-dpu
              {
                system.stateVersion = "25.11";
                fileSystems."/" = {
                  device = "/dev/disk/by-label/nixos";
                  fsType = "ext4";
                };
                fileSystems."/boot/efi" = {
                  device = "/dev/disk/by-label/ESP";
                  fsType = "vfat";
                };
                bluefield.credentials.rootAuthorizedKeys = [ dummyKey ];
              }
            ];
          };
          kexecSystem = lib.nixosSystem {
            system = "aarch64-linux";
            modules = [
              self.nixosModules.bluefield-kexec-installer
              {
                bluefield.credentials.rootAuthorizedKeys = [ dummyKey ];
              }
            ];
          };
          fakeKexecTree = pkgs.runCommand "fake-bluefield-kexec-tree" { } ''
            mkdir -p "$out"
            printf '#!/bin/sh\n' > "$out/kexec-boot"
            chmod 0755 "$out/kexec-boot"
            touch "$out/bzImage" "$out/initrd.gz"
          '';
          fakeKexecTarball = self.lib.mkBluefieldKexecAnywhereTarball {
            inherit pkgs;
            name = "fake-bluefield-kexec-anywhere";
            kexecConfig = {
              system = {
                build.kexecTree = fakeKexecTree;
                nixos.label = "test";
              };
            };
          };
        in
        {
          bluefield-validate = self.packages.${pkgs.stdenv.hostPlatform.system}.bluefield-validate;

          bluefield-module-eval = pkgs.runCommand "bluefield-module-eval" { } ''
            cat > "$out" <<'EOF'
            ${builtins.unsafeDiscardStringContext dpuSystem.config.system.build.toplevel.drvPath}
            ${builtins.unsafeDiscardStringContext kexecSystem.config.system.build.toplevel.drvPath}
            EOF
          '';

          bluefield-validate-kexec-tarball = pkgs.runCommand "bluefield-validate-kexec-tarball"
            {
              nativeBuildInputs = [
                self.packages.${pkgs.stdenv.hostPlatform.system}.bluefield-validate
                pkgs.coreutils
                pkgs.gnutar
                pkgs.xz
              ];
            } ''
            mkdir -p good/kexec bad-link/kexec bad-traversal/kexec bad-owner/kexec bad-mode/kexec bad-space/kexec bad-dot/kexec

            printf '#!/bin/sh\n' > good/kexec/run
            chmod 0755 good/kexec/run
            touch good/kexec/bzImage good/kexec/initrd.gz
            tar --owner=0 --group=0 --numeric-owner -C good -cJf good.tar.xz kexec
            bluefield-validate check-kexec-tarball good.tar.xz

            ln -s /bin/sh bad-link/kexec/run
            touch bad-link/kexec/bzImage bad-link/kexec/initrd.gz
            tar --owner=0 --group=0 --numeric-owner -C bad-link -cJf bad-link.tar.xz kexec
            if bluefield-validate check-kexec-tarball bad-link.tar.xz; then
              echo "validator accepted symlinked kexec/run" >&2
              exit 1
            fi

            printf '#!/bin/sh\n' > bad-traversal/kexec/run
            chmod 0755 bad-traversal/kexec/run
            touch bad-traversal/kexec/bzImage bad-traversal/kexec/initrd.gz
            tar --owner=0 --group=0 --numeric-owner --transform='s#^kexec/run$#../kexec/run#' -C bad-traversal -cJf bad-traversal.tar.xz kexec
            if bluefield-validate check-kexec-tarball bad-traversal.tar.xz; then
              echo "validator accepted path traversal" >&2
              exit 1
            fi

            printf '#!/bin/sh\n' > bad-owner/kexec/run
            chmod 0755 bad-owner/kexec/run
            touch bad-owner/kexec/bzImage bad-owner/kexec/initrd.gz
            tar --owner=123 --group=456 --numeric-owner -C bad-owner -cJf bad-owner.tar.xz kexec
            if bluefield-validate check-kexec-tarball bad-owner.tar.xz; then
              echo "validator accepted non-root ownership" >&2
              exit 1
            fi

            printf '#!/bin/sh\n' > bad-mode/kexec/run
            chmod 0755 bad-mode/kexec/run
            touch bad-mode/kexec/bzImage bad-mode/kexec/initrd.gz
            tar --owner=0 --group=0 --numeric-owner --mode=4755 -C bad-mode -cJf bad-mode.tar.xz kexec
            if bluefield-validate check-kexec-tarball bad-mode.tar.xz; then
              echo "validator accepted setuid mode" >&2
              exit 1
            fi

            printf '#!/bin/sh\n' > 'bad-space/kexec/run extra'
            chmod 0755 'bad-space/kexec/run extra'
            touch bad-space/kexec/bzImage bad-space/kexec/initrd.gz
            tar --owner=0 --group=0 --numeric-owner -C bad-space -cJf bad-space.tar.xz kexec
            if bluefield-validate check-kexec-tarball bad-space.tar.xz; then
              echo "validator accepted ambiguous spaced path as kexec/run" >&2
              exit 1
            fi

            printf '#!/bin/sh\n' > bad-dot/kexec/run
            chmod 0755 bad-dot/kexec/run
            touch bad-dot/kexec/bzImage bad-dot/kexec/initrd.gz
            tar --owner=0 --group=0 --numeric-owner --transform='s#^kexec/run$#./kexec/run#' -C bad-dot -cJf bad-dot.tar.xz kexec
            if bluefield-validate check-kexec-tarball bad-dot.tar.xz; then
              echo "validator accepted dot-prefixed path alias" >&2
              exit 1
            fi

            touch "$out"
          '';

          bluefield-kexec-anywhere-tarball = pkgs.runCommand "bluefield-kexec-anywhere-tarball-check"
            {
              nativeBuildInputs = [ self.packages.${pkgs.stdenv.hostPlatform.system}.bluefield-validate ];
            } ''
            tarball=$(find ${fakeKexecTarball}/tarball -name '*.tar.xz' -print -quit)
            bluefield-validate check-kexec-tarball "$tarball"
            touch "$out"
          '';
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
