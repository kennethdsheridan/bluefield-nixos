{ lib, rustPlatform, makeWrapper, gnutar }:

rustPlatform.buildRustPackage {
  pname = "bluefield-validate";
  version = "0.1.0";

  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [ makeWrapper ];

  postInstall = ''
    wrapProgram "$out/bin/bluefield-validate" \
      --prefix PATH : ${lib.makeBinPath [ gnutar ]}
  '';

  meta = {
    description = "Validation helpers for NVIDIA BlueField NixOS installs";
    license = lib.licenses.mit;
    mainProgram = "bluefield-validate";
  };
}
