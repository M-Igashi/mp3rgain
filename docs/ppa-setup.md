# PPA (Personal Package Archive) Setup Guide

This guide covers the complete setup for distributing mp3rgain and mp3rgui via Ubuntu PPA.

## Overview

The CLI and GUI are distributed through **separate PPAs**:

```bash
# CLI (mp3rgain)
sudo add-apt-repository ppa:m-igashi/mp3rgain
sudo apt update
sudo apt install mp3rgain

# GUI (mp3rgui)
sudo add-apt-repository ppa:m-igashi/mp3rgui
sudo apt update
sudo apt install mp3rgui
```

They are separate so that the GUI, whose vendored dependencies are much larger, can be re-uploaded or fixed without touching a CLI upload that already succeeded.

**Supported platform**: Ubuntu 26.04 LTS (resolute), amd64 and arm64. Older Ubuntu releases (including 24.04 LTS noble) ship Cargo 1.75, below the Rust 1.85 that `symphonia` and `id3` require. Debian and older Ubuntu releases are served by the [apt repository](../README.md#debian-and-ubuntu-apt-repository) published with each GitHub release, which ships the prebuilt `.deb` files instead of building on Launchpad.

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

### 5. Create PPAs

Create two PPAs, one for the CLI and one for the GUI.

**PPA 1: mp3rgain (CLI)**
1. Go to https://launchpad.net/~/+activate-ppa
2. PPA name: `mp3rgain`
3. Display name: `mp3rgain - Lossless MP3 volume adjustment`
4. Click "Activate"

**PPA 2: mp3rgui (GUI)**
1. Go to https://launchpad.net/~/+activate-ppa
2. PPA name: `mp3rgui`
3. Display name: `mp3rgui - GUI for mp3rgain`
4. Click "Activate"

The URLs will be: `ppa:m-igashi/mp3rgain` and `ppa:m-igashi/mp3rgui`.

### 6. Enable arm64 Processors

A new PPA builds for amd64 only. To build for arm64 as well, open each PPA's admin edit page and check `arm64`:

- https://launchpad.net/~m-igashi/+archive/ubuntu/mp3rgain/+edit
- https://launchpad.net/~m-igashi/+archive/ubuntu/mp3rgui/+edit

Under **Processors**, enable at minimum:
- `AMD x86-64 (amd64)`
- `ARM ARMv8 (arm64)`

### 7. Register an SSH Key on Launchpad

Uploads go over SFTP, authenticated with an SSH key registered on the Launchpad account. Register the public half of the key at https://launchpad.net/~/+editsshkeys.

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
# Build both packages for every supported series (resolute)
./scripts/build-ppa.sh

# Build only the CLI for resolute
./scripts/build-ppa.sh --package=cli --distro=resolute

# Build and upload both, each to its own PPA
./scripts/build-ppa.sh --upload

# Specify GPG key
./scripts/build-ppa.sh --package=cli --key=YOUR_KEY_ID --upload

# Dry run (show what would be built and where it would be uploaded)
./scripts/build-ppa.sh --dry-run --upload
```

### Script Options

| Option | Description |
|--------|-------------|
| `--upload` | Upload to PPA after building |
| `--package=PKG` | `cli`, `gui`, or `all` (default: `all`) |
| `--distro=DISTRO` | Build for one series only (default: every series in `ALL_DISTROS`, currently `resolute`) |
| `--ppa-cli=PPA` | Upload target for mp3rgain (default: `ppa:m-igashi/mp3rgain`), like the workflow's `target_ppa_cli` |
| `--ppa-gui=PPA` | Upload target for mp3rgui (default: `ppa:m-igashi/mp3rgui`), like the workflow's `target_ppa_gui` |
| `--key=KEYID` | GPG key ID for signing |
| `--dry-run` | Show commands without executing |

### Supported Ubuntu Releases

| Codename | Version | Status | Notes |
|----------|---------|--------|-------|
| resolute | 26.04 LTS | Supported (default) | rustc 1.93, comfortably above the 1.85 MSRV |
| stonking | 26.10 | Should work, untested | In pre-release freeze as of 2026-10-08; rustc 1.97 |
| questing | 25.10 | **Dead** | End of life. Launchpad rejects uploads: "questing is obsolete and will not accept new uploads" |
| noble | 24.04 LTS | Not supported | Cargo 1.75, below the 1.85 MSRV |

### What the Script Does

1. Exports clean source from git (`git archive HEAD`)
2. Converts `Cargo.lock` v4 to v3, then, in the vendored crates, downgrades `edition = "2024"` to `"2021"` and strips `rust-version` lines; the `checksum` lines are removed from `Cargo.lock` after vendoring. These are compatibility shims for older distros and no-ops on resolute
3. Runs `cargo vendor` to bundle all Rust dependencies
4. Removes prebuilt Windows/macOS libraries (`.a`, `.dll`, `.lib`) and the tests, benches and extra Markdown files from the vendored crates, and clears their checksums
5. Creates `.cargo/config.toml` for offline builds
6. Creates the `.orig.tar.xz` (source + vendored deps)
7. Generates `debian/changelog` for each Ubuntu release
8. Builds signed source packages with `debuild -S -sa`
9. Optionally uploads with `dput`

The workflow does the same, plus two things the script does not: for the GUI it stubs out Windows- and macOS-only crates (`windows-*`, `objc*`, `cocoa-*`, `metal-*` and similar) and compresses the tarball with `xz -9e`, both to keep the orig tarball small.

### Manual Upload

Anonymous upload to `ppa.launchpad.net`, the stock `dput` path, no longer works. Uploads go over SFTP with the SSH key from step 7, so put the same stanza the workflow uses in `~/.dput.cf` (`--upload` needs it too):

```ini
[ppa]
fqdn = ppa.launchpad.net
method = sftp
incoming = ~%(ppa)s/ubuntu/
login = m-igashi
allow_unsigned_uploads = 0
```

`login` is the Launchpad username. Then, if you built without `--upload`:

```bash
dput ppa:m-igashi/mp3rgain build-ppa/mp3rgain/build-resolute/mp3rgain_*_source.changes
dput ppa:m-igashi/mp3rgui  build-ppa/mp3rgui/build-resolute/mp3rgui_*_source.changes
```

## After Upload

### Check Build Status

1. Go to https://launchpad.net/~m-igashi/+archive/ubuntu/mp3rgain (CLI) or https://launchpad.net/~m-igashi/+archive/ubuntu/mp3rgui (GUI)
2. Click on the package name to see build status
3. Builds typically take 10-30 minutes

Use the PPA pages rather than the Launchpad API: `getBuildRecords` leaves out builds that are still in "Pending publication", so an empty API response does not mean nothing was built.

### If Build Fails

1. Check the build log on Launchpad
2. Common issues:
   - Missing Build-Depends → update `debian/control`
   - Vendored deps incomplete → re-run `cargo vendor`
   - Architecture-specific issues → check build log details

## Using Docker for Builds (from macOS)

Since `debuild` requires Linux, you can use Docker:

```bash
docker run --rm -it -v "$(pwd):/workspace" ubuntu:26.04 bash

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
  ubuntu:26.04 bash
```

## Release Workflow

PPA upload is **automatic**. When you push a release tag:

1. Release workflow builds binaries and creates GitHub Release
2. On success, the PPA workflow (`.github/workflows/ppa.yml`) triggers automatically
3. Source packages are built, signed, and uploaded to Launchpad
4. Launchpad builds .deb packages for amd64 and arm64

After a release, the PPA workflow checks out the commit the Release workflow built (the tagged commit), so the version comes from that commit's `Cargo.toml` and the orig tarball is the tagged tree. Commits pushed to `master` in the meantime, such as the AUR update the Release workflow pushes itself, are not included. A manual run builds `master` as it is when the run starts.

To manually trigger: Actions → PPA Upload → Run workflow. The inputs choose the package (`all`, `cli` or `gui`), the PPA revision, the two target PPAs and the target series (default `resolute`).

The workflow needs three secrets. They are stored as secrets of the GitHub environment `ppa`, which both upload jobs declare with `environment: ppa` and which only the `master` branch can use:

| Secret | Contents |
|--------|----------|
| `GPG_PRIVATE_KEY` | base64-encoded GPG private key (`gpg --export-secret-keys KEY_ID \| base64`) |
| `GPG_PASSPHRASE` | Passphrase for the GPG key |
| `LAUNCHPAD_SSH_PRIVATE_KEY` | OpenSSH-format private key (ed25519 or rsa) whose public half is registered at https://launchpad.net/~m-igashi/+editsshkeys |

## Troubleshooting

### "Signature could not be verified"

Your GPG key isn't registered on Launchpad, or the email doesn't match.

### "Already uploaded" / "different contents"

Each version's orig tarball can only be uploaded once. Options:
- Bump PPA revision via `ppa_revision` input (e.g., `2`)
- If orig tarball contents changed, delete and recreate the PPA

### "lock file version 4 requires -Znext-lockfile-bump"

Ubuntu noble's cargo (Rust 1.75) doesn't support Cargo.lock v4. This is one of several reasons we target resolute (26.04 LTS) instead. The workflow still runs the v4→v3 conversion for compatibility.

### "Build-Depends not satisfiable"

A build dependency isn't available in that Ubuntu release. Check package availability:
```bash
rmadison -u ubuntu <package-name>
```

### "obsolete and will not accept new uploads"

The Ubuntu release has reached EOL and Launchpad will never accept it again. Move to a supported series: change the `target_distro` default and the `|| 'resolute'` fallbacks in `.github/workflows/ppa.yml`, and `ALL_DISTROS` in `scripts/build-ppa.sh`. To list the active series and check that a series' rustc meets the MSRV:

```bash
curl -s https://api.launchpad.net/devel/ubuntu/series | \
  python3 -c "import sys,json;[print(e['name'],e['version'],e['status']) for e in json.load(sys.stdin)['entries'] if e.get('active')]"
curl -s https://packages.ubuntu.com/<series>/rustc | grep -oE 'Package: rustc \(([^)]+)\)'
```
