{ lib, rustPlatform }:

rustPlatform.buildRustPackage {
  pname = "bluefield-validate";
  version = "0.1.0";

  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;

  meta = {
    description = "Validation helpers for NVIDIA BlueField NixOS installs";
    license = lib.licenses.mit;
    mainProgram = "bluefield-validate";
  };
}
