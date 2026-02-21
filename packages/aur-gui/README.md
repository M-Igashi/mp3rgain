# mp3rgui AUR Package

This directory contains the PKGBUILD for the **mp3rgui** (GUI application) Arch Linux User Repository (AUR) package.

For the CLI tool (`mp3rgain`), see `../aur/`.

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
- **Optional**: `mp3rgain` (CLI tool for batch processing)

## Publishing to AUR (for maintainers)

1. Clone the AUR repository:
   ```bash
   git clone ssh://aur@aur.archlinux.org/mp3rgui.git aur-mp3rgui
   cd aur-mp3rgui
   ```

2. Copy the package files:
   ```bash
   cp /path/to/mp3rgain/packages/aur-gui/PKGBUILD .
   cp /path/to/mp3rgain/packages/aur-gui/.SRCINFO .
   cp /path/to/mp3rgain/packages/aur-gui/mp3rgui.desktop .
   ```

3. Update the sha256sum in PKGBUILD:
   ```bash
   updpkgsums
   ```

4. Regenerate .SRCINFO:
   ```bash
   makepkg --printsrcinfo > .SRCINFO
   ```

5. Test the build:
   ```bash
   makepkg -si
   ```

6. Commit and push:
   ```bash
   git add PKGBUILD .SRCINFO mp3rgui.desktop
   git commit -m "Update to version X.Y.Z"
   git push
   ```

## Version Updates

When releasing a new version:

1. Update `pkgver` in PKGBUILD
2. Reset `pkgrel` to 1
3. Update sha256sum with `updpkgsums`
4. Regenerate .SRCINFO
5. Test build locally
6. Push to AUR
