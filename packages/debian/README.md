# mp3rgain Debian Package

This directory contains the Debian packaging files for building `.deb` packages.

## Building the Package

### Prerequisites

```bash
# Install build dependencies
sudo apt-get update
sudo apt-get install -y build-essential debhelper
```

Rust 1.85 or later is also required. The `rustc` in Debian 12 and Ubuntu 24.04 is older, so install it with [rustup](https://rustup.rs) as the release workflow does. The `cargo` / `rustc` Build-Depends are then not Debian packages, which is why the build commands below pass `-d`.

### Build Steps

1. Clone the repository and navigate to the project root:
   ```bash
   git clone https://github.com/M-Igashi/mp3rgain.git
   cd mp3rgain
   ```

2. Copy the debian directory to the project root:
   ```bash
   cp -r packages/debian/debian .
   ```

3. Build the package:
   ```bash
   dpkg-buildpackage -us -uc -b -d
   ```

4. The `.deb` file will be created in the parent directory:
   ```bash
   ls ../*.deb
   ```

### Installing the Package

```bash
# x86_64
sudo dpkg -i ../mp3rgain_*-1_amd64.deb
# ARM64
sudo dpkg -i ../mp3rgain_*-1_arm64.deb
```

### Using Docker (recommended for clean builds)

```bash
# Build using Docker
docker run --rm -v "$(pwd):/src" -w /src rust:1 bash -c '
  apt-get update && \
  apt-get install -y build-essential debhelper && \
  cp -r packages/debian/debian . && \
  dpkg-buildpackage -us -uc -b -d && \
  cp ../mp3rgain_*.deb /src/
'
```

## GitHub Actions Build

The release workflow builds the `.deb` for amd64 and arm64 on every release tag and attaches it to the GitHub release as `mp3rgain_X.Y.Z-1_amd64.deb` and `mp3rgain_X.Y.Z-1_arm64.deb`, each with a `.sha256`.

The GUI has its own package (`mp3rgui`), built the same way from `packages/debian-gui/`.

After the release workflow, `.github/workflows/apt.yml` indexes those `.deb` files into a signed flat apt repository (`Packages`, `Release`, `InRelease`, the public key) and uploads it to the same release, so users can install and update through apt from `releases/latest/download/`. Setup is in the [README](../../README.md#debian-and-ubuntu-apt-repository).

## Version Updates

The release workflow overwrites `debian/changelog` with the release version before building, so no `dch` step is needed. The committed `changelog` is a placeholder; a local build takes its package version from it unless you edit the copied `debian/changelog` first.

## File Descriptions

- `control`: Package metadata and dependencies
- `rules`: Build instructions (Makefile)
- `changelog`: Version history
- `copyright`: License information
- `source/format`: Source package format
