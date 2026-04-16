# PPA (Personal Package Archive) Setup Guide

This guide covers the complete setup for distributing mp3rgain and mp3rgui via Ubuntu PPA.

## Overview

PPA allows Ubuntu users to install mp3rgain with:

```bash
sudo add-apt-repository ppa:m-igashi/mp3rgain
sudo apt update
sudo apt install mp3rgain mp3rgui
```

## Prerequisites

### 1. Create a Launchpad Account

1. Go to https://launchpad.net/ and click "Log in / Register"
2. Create an Ubuntu One account (or use existing one)
3. Log in to Launchpad

### 2. Generate a GPG Key

If you don't already have a GPG key:

```bash
gpg --full-generate-key
```

- Choose: RSA and RSA
- Key size: 4096
- Expiration: 2y (recommended)
- Name: M-Igashi
- Email: (same as Launchpad account)

List your key:

```bash
gpg --list-keys --keyid-format long
```

Note the key ID (e.g., `ABCDEF1234567890`).

### 3. Upload GPG Key to Ubuntu Keyserver

```bash
gpg --keyserver keyserver.ubuntu.com --send-keys YOUR_KEY_ID
```

### 4. Register GPG Key on Launchpad

1. Go to https://launchpad.net/~/+editpgpkeys
2. Paste your full fingerprint: `gpg --fingerprint YOUR_KEY_ID`
3. Click "Import Key"
4. Check your email for the encrypted confirmation message
5. Decrypt and follow the confirmation link:
   ```bash
   gpg --decrypt confirmation.txt
   ```

### 5. Create PPA

1. Go to https://launchpad.net/~/+activate-ppa
2. PPA name: `mp3rgain`
3. Display name: `mp3rgain - Lossless MP3 volume adjustment`
4. Description:
   ```
   Modern mp3gain replacement written in Rust.
   Includes mp3rgain (CLI) and mp3rgui (GUI).
   
   Homepage: https://github.com/M-Igashi/mp3rgain
   ```
5. Click "Activate"

The PPA URL will be: `ppa:m-igashi/mp3rgain`

## Building and Uploading Packages

### Build Environment

Building PPA source packages requires an Ubuntu machine (or Docker container).

**Install dependencies:**

```bash
sudo apt install devscripts debhelper dput gpg cargo rustc
```

### Build Source Packages

From the project root:

```bash
# Build for all Ubuntu releases (both CLI and GUI)
./scripts/build-ppa.sh

# Build only CLI for noble
./scripts/build-ppa.sh --package=cli --distro=noble

# Build and upload
./scripts/build-ppa.sh --upload

# Specify GPG key
./scripts/build-ppa.sh --key=YOUR_KEY_ID --upload

# Dry run (show what would be done)
./scripts/build-ppa.sh --dry-run
```

### Script Options

| Option | Description |
|--------|-------------|
| `--upload` | Upload to PPA after building |
| `--package=PKG` | `cli`, `gui`, or `all` (default: `all`) |
| `--distro=DISTRO` | Build for specific distro (default: all supported) |
| `--ppa=PPA` | PPA target (default: `ppa:m-igashi/mp3rgain`) |
| `--key=KEYID` | GPG key ID for signing |
| `--dry-run` | Show commands without executing |

### Supported Ubuntu Releases

| Codename | Version | Status |
|----------|---------|--------|
| noble | 24.04 LTS | Supported |
| oracular | 24.10 | Supported |
| plucky | 25.04 | Supported |

### What the Script Does

1. Exports clean source from git
2. Runs `cargo vendor` to bundle all Rust dependencies
3. Creates `.cargo/config.toml` for offline builds
4. Creates `.orig.tar.gz` (source + vendored deps)
5. Generates `debian/changelog` for each Ubuntu release
6. Builds signed source packages with `debuild -S`
7. Optionally uploads with `dput`

### Manual Upload

If you built without `--upload`:

```bash
dput ppa:m-igashi/mp3rgain build-ppa/mp3rgain/build-noble/mp3rgain_*_source.changes
dput ppa:m-igashi/mp3rgain build-ppa/mp3rgui/build-noble/mp3rgui_*_source.changes
```

## After Upload

### Check Build Status

1. Go to https://launchpad.net/~m-igashi/+archive/ubuntu/mp3rgain
2. Click on the package name to see build status
3. Builds typically take 10-30 minutes

### If Build Fails

1. Check the build log on Launchpad
2. Common issues:
   - Missing Build-Depends → update `debian/control`
   - Vendored deps incomplete → re-run `cargo vendor`
   - Architecture-specific issues → check build log details

## Using Docker for Builds (from macOS)

Since `debuild` requires Linux, you can use Docker:

```bash
docker run --rm -it -v "$(pwd):/workspace" ubuntu:24.04 bash

# Inside container:
apt update && apt install -y devscripts debhelper dput gpg cargo rustc git
cd /workspace
./scripts/build-ppa.sh --dry-run
```

For signing, you'll need to mount your GPG key:

```bash
docker run --rm -it \
  -v "$(pwd):/workspace" \
  -v "$HOME/.gnupg:/root/.gnupg" \
  ubuntu:24.04 bash
```

## Release Workflow

When releasing a new version:

1. Update version in `Cargo.toml` and `mp3rgui/Cargo.toml`
2. Create git tag and push (triggers GitHub release workflow)
3. Build and upload PPA packages:
   ```bash
   ./scripts/build-ppa.sh --upload
   ```
4. Verify builds on Launchpad

## Troubleshooting

### "Signature could not be verified"

Your GPG key isn't registered on Launchpad, or the email doesn't match.

### "Already uploaded"

Each version can only be uploaded once per distro. Bump the PPA revision:
- Change `0ppa1` to `0ppa2` in the build script (or modify the changelog manually)

### "Build-Depends not satisfiable"

A build dependency isn't available in that Ubuntu release. Check package availability:
```bash
rmadison -u ubuntu <package-name>
```
