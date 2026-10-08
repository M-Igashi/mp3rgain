# mp3rgui AUR Package

This directory contains the PKGBUILD for the **mp3rgui** (GUI application) Arch Linux User Repository (AUR) package.

For the CLI tool (`mp3rgain`), install `mp3rgain-bin`, which a third-party packager maintains on AUR.

## Installation (for users)

```bash
# Using yay
yay -S mp3rgui

# Using paru
paru -S mp3rgui

# Manual installation
git clone https://aur.archlinux.org/mp3rgui.git
cd mp3rgui
makepkg -si
```

## Dependencies

- **Runtime**: `gcc-libs`, `gtk3` (for native file dialogs)
- **Build**: `rust`, `cargo`
- **Optional**: `mp3rgain` (CLI tool for batch processing), `noto-fonts-cjk` (CJK characters in file names)

## Publishing to AUR (for maintainers)

The `update-aur` job in `.github/workflows/release.yml` publishes every release: it sets `pkgver` and resets `pkgrel` to 1 in `PKGBUILD` and `.SRCINFO`, commits that to `master`, and pushes `PKGBUILD`, `.SRCINFO` and `mp3rgui.desktop` to the AUR repository. `sha256sums` is `SKIP`, so there is no checksum to update.

The job only rewrites the version fields, so any other `PKGBUILD` change (dependencies, build steps) needs the matching edit in `.SRCINFO` too.

### Manual fallback

The job is `continue-on-error`. If it fails, bump `pkgver` in both files if it did not get that far, then push by hand:

```bash
git clone ssh://aur@aur.archlinux.org/mp3rgui.git aur-mp3rgui
cd aur-mp3rgui
cp /path/to/mp3rgain/packages/aur-gui/{PKGBUILD,.SRCINFO,mp3rgui.desktop} .
makepkg -si   # test the build
git add PKGBUILD .SRCINFO mp3rgui.desktop
git commit -m "Update to version X.Y.Z"
git push
```
