# mp3rgain Debian Package

This directory contains the Debian packaging files for building `.deb` packages.

## Building the Package

### Prerequisites

```bash
# Install build dependencies
sudo apt-get update
sudo apt-get install -y build-essential debhelper cargo rustc
```

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
   dpkg-buildpackage -us -uc -b
   ```

4. The `.deb` file will be created in the parent directory:
   ```bash
   ls ../*.deb
   ```

### Installing the Package

```bash
sudo dpkg -i ../mp3rgain_1.1.1-1_amd64.deb
```

### Using Docker (recommended for clean builds)

```bash
# Build using Docker
docker run --rm -v "$(pwd):/src" -w /src debian:bookworm bash -c '
  apt-get update && \
  apt-get install -y build-essential debhelper cargo rustc && \
  cp -r packages/debian/debian . && \
  dpkg-buildpackage -us -uc -b
'
```

## GitHub Actions Build

The `.deb` package is automatically built on each release. Download the artifact from the GitHub Actions workflow or the release assets.

## Version Updates

When releasing a new version:

1. Update `packages/debian/debian/changelog`:
   ```bash
   dch -v NEW_VERSION-1 "Release NEW_VERSION"
   ```

2. Update the version in `Cargo.toml`

3. Rebuild the package

## File Descriptions

- `control`: Package metadata and dependencies
- `rules`: Build instructions (Makefile)
- `changelog`: Version history
- `copyright`: License information
- `compat`: Debhelper compatibility level
- `source/format`: Source package format
