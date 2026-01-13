# mp3rgain AUR Package

This directory contains the PKGBUILD for the Arch Linux User Repository (AUR).

## Installation (for users)

```bash
# Using yay
yay -S mp3rgain

# Using paru
paru -S mp3rgain

# Manual installation
git clone https://aur.archlinux.org/mp3rgain.git
cd mp3rgain
makepkg -si
```

## Publishing to AUR (for maintainers)

1. Clone the AUR repository:
   ```bash
   git clone ssh://aur@aur.archlinux.org/mp3rgain.git aur-mp3rgain
   cd aur-mp3rgain
   ```

2. Copy the PKGBUILD and .SRCINFO:
   ```bash
   cp /path/to/mp3rgain/packages/aur/PKGBUILD .
   cp /path/to/mp3rgain/packages/aur/.SRCINFO .
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
   git add PKGBUILD .SRCINFO
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
