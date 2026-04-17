#!/usr/bin/env bash
#
# Build PPA source packages for mp3rgain and mp3rgui.
#
# This script:
#   1. Vendors all Rust dependencies (for offline build on Launchpad)
#   2. Creates orig tarballs
#   3. Builds signed source packages for each Ubuntu release
#   4. Optionally uploads to Launchpad PPA via dput
#
# Prerequisites (Ubuntu):
#   sudo apt install devscripts debhelper dput gpg cargo rustc
#
# Usage:
#   ./scripts/build-ppa.sh [options]
#
# Options:
#   --upload          Upload to PPA after building
#   --package=PKG     Build only 'cli', 'gui', or 'all' (default: all)
#   --distro=DISTRO   Build for specific distro only (default: all supported)
#   --ppa=PPA         PPA target (default: ppa:m-igashi/mp3rgain)
#   --key=KEYID       GPG key ID for signing (default: auto-detect)
#   --dry-run         Show what would be done without executing
#   --help            Show this help message

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_DIR="${PROJECT_DIR}/build-ppa"

# Supported Ubuntu releases
ALL_DISTROS=("noble")

# Defaults
UPLOAD=false
PACKAGE="all"
DISTROS=("${ALL_DISTROS[@]}")
PPA="ppa:m-igashi/mp3rgain"
GPG_KEY=""
DRY_RUN=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --upload) UPLOAD=true; shift ;;
        --package=*) PACKAGE="${1#*=}"; shift ;;
        --distro=*) DISTROS=("${1#*=}"); shift ;;
        --ppa=*) PPA="${1#*=}"; shift ;;
        --key=*) GPG_KEY="${1#*=}"; shift ;;
        --dry-run) DRY_RUN=true; shift ;;
        --help) head -20 "$0" | tail -15; exit 0 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# Read version from Cargo.toml
VERSION=$(grep '^version' "$PROJECT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')
echo "==> mp3rgain version: $VERSION"
echo "==> Target distros: ${DISTROS[*]}"
echo "==> Package: $PACKAGE"
echo "==> PPA: $PPA"
echo ""

if $DRY_RUN; then
    echo "[DRY RUN] No changes will be made."
    echo ""
fi

# Clean previous builds
if [[ -d "$BUILD_DIR" ]] && ! $DRY_RUN; then
    echo "==> Cleaning previous build directory..."
    rm -rf "$BUILD_DIR"
fi

build_source_package() {
    local pkg_name="$1"     # mp3rgain or mp3rgui
    local ppa_dir="$2"      # packages/ppa or packages/ppa-gui
    local orig_version="$VERSION"

    echo ""
    echo "=========================================="
    echo "  Building source package: $pkg_name"
    echo "=========================================="

    local pkg_build_dir="$BUILD_DIR/$pkg_name"
    mkdir -p "$pkg_build_dir"

    # Step 1: Create source directory with vendored dependencies
    echo "==> Creating source directory..."
    local src_dir="$pkg_build_dir/${pkg_name}-${orig_version}"
    mkdir -p "$src_dir"

    # Export source from git (clean, no .git directory)
    git -C "$PROJECT_DIR" archive HEAD | tar -x -C "$src_dir"

    # Step 2: Convert Cargo.lock v4 to v3 for Ubuntu noble compatibility
    echo "==> Converting Cargo.lock to v3 format..."
    python3 -c "
import re, glob
for lockfile in glob.glob('$src_dir/**/Cargo.lock', recursive=True):
    with open(lockfile) as f: c = f.read()
    if 'version = 4' not in c: continue
    c = c.replace('version = 4', 'version = 3', 1)
    c = re.sub(r'source = \"([^\"#]+)#([^\"]+)\"', r'source = \"\1\"\nchecksum = \"\2\"', c)
    with open(lockfile, 'w') as f: f.write(c)
    print(f'  Converted: {lockfile}')
"

    # Step 3: Vendor Rust dependencies
    echo "==> Vendoring Rust dependencies..."
    if [[ "$pkg_name" == "mp3rgui" ]]; then
        (cd "$src_dir" && cargo vendor --manifest-path mp3rgui/Cargo.toml vendor)
        # Strip checksum lines from Cargo.lock after vendor (cleared cargo-checksum.json invalidates them)
        (cd "$src_dir" && sed -i '/^checksum = /d' Cargo.lock mp3rgui/Cargo.lock)
    else
        (cd "$src_dir" && cargo vendor vendor)
        (cd "$src_dir" && sed -i '/^checksum = /d' Cargo.lock)
    fi

    # Step 3: Create .cargo/config.toml for offline builds
    mkdir -p "$src_dir/.cargo"
    cat > "$src_dir/.cargo/config.toml" << 'CARGO_CONFIG'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
CARGO_CONFIG

    # Step 4: Clean up vendored deps to reduce tarball size
    echo "==> Cleaning vendored dependencies..."
    find "$src_dir/vendor" -name '*.a' -delete 2>/dev/null || true
    find "$src_dir/vendor" -name '*.dll' -delete 2>/dev/null || true
    find "$src_dir/vendor" -name '*.lib' -delete 2>/dev/null || true
    find "$src_dir/vendor" -type d \( -name tests -o -name benches -o -name .github \) -exec rm -rf {} + 2>/dev/null || true
    find "$src_dir/vendor" -name '*.md' ! -name 'README.md' -delete 2>/dev/null || true
    # Downgrade edition2024 to edition2021 and strip rust-version for Ubuntu noble's Cargo 1.75.0
    find "$src_dir/vendor" -name Cargo.toml -exec sed -i -e 's/^edition = "2024"$/edition = "2021"/' -e 's/^resolver = "3"$/resolver = "2"/' -e '/^rust-version = /d' {} +
    # Clear cargo checksums (standard practice for Debian Rust packaging)
    for f in "$src_dir"/vendor/*/.cargo-checksum.json; do echo '{"files":{}}' > "$f"; done

    # Step 5: Remove files not needed in the source package
    rm -rf "$src_dir/.claude" "$src_dir/.github" "$src_dir/target" "$src_dir/CLAUDE.md"

    # Step 6: Create orig tarball (xz for better compression)
    echo "==> Creating orig tarball..."
    local orig_tarball="${pkg_build_dir}/${pkg_name}_${orig_version}.orig.tar.xz"
    tar -cJf "$orig_tarball" -C "$pkg_build_dir" "${pkg_name}-${orig_version}"

    # Step 6: Build source package for each distro
    for distro in "${DISTROS[@]}"; do
        echo ""
        echo "--- Building for $distro ---"

        local work_dir="$pkg_build_dir/build-${distro}"
        mkdir -p "$work_dir"

        # Extract orig tarball and copy it to parent dir (required by dpkg-source)
        tar -xJf "$orig_tarball" -C "$work_dir"
        cp "$orig_tarball" "$work_dir/"
        local build_src="$work_dir/${pkg_name}-${orig_version}"

        # Copy PPA-specific debian directory
        cp -r "$PROJECT_DIR/$ppa_dir/debian" "$build_src/debian"

        # Generate changelog for this distro
        local ppa_version="${orig_version}-0ppa1~${distro}1"
        local date_rfc2822
        date_rfc2822=$(date -R)

        cat > "$build_src/debian/changelog" << CHANGELOG
${pkg_name} (${ppa_version}) ${distro}; urgency=medium

  * Release ${orig_version}

 -- M-Igashi <M-Igashi@users.noreply.github.com>  ${date_rfc2822}
CHANGELOG

        # Build signed source package
        echo "==> Building source package for $distro..."
        local debuild_opts="-S -sa"
        if [[ -n "$GPG_KEY" ]]; then
            debuild_opts="$debuild_opts -k${GPG_KEY}"
        fi

        if $DRY_RUN; then
            echo "[DRY RUN] Would run: debuild $debuild_opts (in $build_src)"
        else
            (cd "$build_src" && debuild $debuild_opts)
        fi

        # Upload if requested
        if $UPLOAD; then
            local changes_file="$work_dir/${pkg_name}_${ppa_version}_source.changes"
            if $DRY_RUN; then
                echo "[DRY RUN] Would run: dput $PPA $changes_file"
            else
                echo "==> Uploading $pkg_name for $distro to $PPA..."
                dput "$PPA" "$changes_file"
            fi
        fi
    done

    echo ""
    echo "==> $pkg_name source packages built successfully!"
}

# Main
if $DRY_RUN; then
    echo "[DRY RUN mode]"
    echo ""
fi

if [[ "$PACKAGE" == "cli" || "$PACKAGE" == "all" ]]; then
    if $DRY_RUN; then
        echo "[DRY RUN] Would build mp3rgain CLI source package"
    else
        build_source_package "mp3rgain" "packages/ppa"
    fi
fi

if [[ "$PACKAGE" == "gui" || "$PACKAGE" == "all" ]]; then
    if $DRY_RUN; then
        echo "[DRY RUN] Would build mp3rgui GUI source package"
    else
        build_source_package "mp3rgui" "packages/ppa-gui"
    fi
fi

echo ""
echo "=========================================="
echo "  Done!"
echo "=========================================="
echo ""
echo "Build artifacts are in: $BUILD_DIR"

if ! $UPLOAD; then
    echo ""
    echo "To upload to PPA:"
    echo "  dput $PPA $BUILD_DIR/<package>/build-<distro>/<package>_*_source.changes"
fi
