# mp3rgain Nix Package

This directory contains the Nix package expression for mp3rgain.

## Installation

### Using Flakes (recommended)

```bash
# Run directly without installing
nix run github:M-Igashi/mp3rgain

# Install to profile
nix profile install github:M-Igashi/mp3rgain

# Add to flake.nix
{
  inputs.mp3rgain.url = "github:M-Igashi/mp3rgain";
}
```

### Using nix-shell

```bash
nix-shell -p '(import (fetchTarball "https://github.com/M-Igashi/mp3rgain/archive/master.tar.gz") {}).packages.${builtins.currentSystem}.default'
```

### NixOS Configuration

Add to your `configuration.nix`:

```nix
{ config, pkgs, ... }:

let
  mp3rgain = pkgs.callPackage (pkgs.fetchFromGitHub {
    owner = "M-Igashi";
    repo = "mp3rgain";
    rev = "v1.1.1";
    hash = "sha256-...";
  } + "/packages/nix/default.nix") { };
in
{
  environment.systemPackages = [ mp3rgain ];
}
```

## Submitting to nixpkgs

To submit this package to the official nixpkgs repository:

1. Fork [nixpkgs](https://github.com/NixOS/nixpkgs)

2. Add the package to `pkgs/by-name/mp/mp3rgain/package.nix`:
   ```nix
   { lib
   , rustPlatform
   , fetchFromGitHub
   }:

   rustPlatform.buildRustPackage rec {
     pname = "mp3rgain";
     version = "1.1.1";

     src = fetchFromGitHub {
       owner = "M-Igashi";
       repo = "mp3rgain";
       rev = "v${version}";
       hash = "sha256-...";
     };

     cargoHash = "sha256-...";

     meta = with lib; {
       description = "Lossless MP3 volume adjustment - a modern mp3gain replacement written in Rust";
       homepage = "https://github.com/M-Igashi/mp3rgain";
       changelog = "https://github.com/M-Igashi/mp3rgain/releases/tag/v${version}";
       license = licenses.mit;
       maintainers = with maintainers; [ ];
       mainProgram = "mp3rgain";
       platforms = platforms.all;
     };
   }
   ```

3. Build and test:
   ```bash
   nix-build -A mp3rgain
   ./result/bin/mp3rgain --version
   ```

4. Submit a pull request following the [nixpkgs contribution guide](https://github.com/NixOS/nixpkgs/blob/master/CONTRIBUTING.md)

## Development

Enter a development shell:

```bash
nix develop
```

Build the package locally:

```bash
nix build
./result/bin/mp3rgain --version
```
