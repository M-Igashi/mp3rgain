{
  description = "mp3rgain - Lossless MP3 volume adjustment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        mp3rgain = pkgs.rustPlatform.buildRustPackage {
          pname = "mp3rgain";
          version = cargoToml.package.version;

          src = self;
          cargoLock.lockFile = ./Cargo.lock;

          meta = {
            description = "Lossless MP3 volume adjustment - a modern mp3gain replacement written in Rust";
            homepage = "https://github.com/M-Igashi/mp3rgain";
            changelog = "https://github.com/M-Igashi/mp3rgain/releases/tag/v${cargoToml.package.version}";
            license = pkgs.lib.licenses.mit;
            mainProgram = "mp3rgain";
            platforms = pkgs.lib.platforms.all;
          };
        };
      in
      {
        packages.default = mp3rgain;
        packages.mp3rgain = mp3rgain;

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            rust-analyzer
            clippy
            rustfmt
          ];
        };
      });
}
