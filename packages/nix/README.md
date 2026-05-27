# mp3rgain Nix Package

mp3rgain ships a flake at the **repository root** (`flake.nix` / `flake.lock`).
It builds straight from the checked-out source (`src = self`) and pins
dependencies from the committed `Cargo.lock`, so there are no per-release
hashes to maintain.

## Installation

### Run without installing

```bash
nix run github:M-Igashi/mp3rgain -- --help
```

### Install into your profile

```bash
nix profile install github:M-Igashi/mp3rgain
```

### Use as a flake input

```nix
{
  inputs.mp3rgain.url = "github:M-Igashi/mp3rgain";

  # In a NixOS module / home-manager config:
  #   environment.systemPackages = [ mp3rgain.packages.${pkgs.system}.default ];
}
```

## Development

```bash
nix develop          # shell with cargo, rustc, clippy, rustfmt, rust-analyzer
nix build            # build the package
./result/bin/mp3rgain --version
```

## Submitting to nixpkgs

The root flake is for installing directly from GitHub. A nixpkgs package pins a
release tarball by hash instead. To submit:

1. Fork [nixpkgs](https://github.com/NixOS/nixpkgs) and add
   `pkgs/by-name/mp/mp3rgain/package.nix`:

   ```nix
   { lib, rustPlatform, fetchFromGitHub }:

   rustPlatform.buildRustPackage rec {
     pname = "mp3rgain";
     version = "2.7.2";

     src = fetchFromGitHub {
       owner = "M-Igashi";
       repo = "mp3rgain";
       rev = "v${version}";
       hash = lib.fakeHash;    # nix-prefetch-github M-Igashi mp3rgain --rev v${version}
     };

     cargoHash = lib.fakeHash; # replaced by the value Nix prints on first build

     meta = {
       description = "Lossless MP3 volume adjustment - a modern mp3gain replacement written in Rust";
       homepage = "https://github.com/M-Igashi/mp3rgain";
       changelog = "https://github.com/M-Igashi/mp3rgain/releases/tag/v${version}";
       license = lib.licenses.mit;
       mainProgram = "mp3rgain";
       platforms = lib.platforms.all;
     };
   }
   ```

2. Replace both `lib.fakeHash` placeholders with the real values (each prints a
   hash-mismatch error on first build that reveals the correct hash), build with
   `nix-build -A mp3rgain`, then open a PR per the
   [nixpkgs contribution guide](https://github.com/NixOS/nixpkgs/blob/master/CONTRIBUTING.md).
```
