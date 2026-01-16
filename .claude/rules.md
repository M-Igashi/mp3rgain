# mp3rgain Project Rules

## Internal Directories and Files

The following directories contain internal/generated content:

### Never commit to git (in .gitignore)
- `target/` - Rust build artifacts

### Excluded from crates.io package (in Cargo.toml exclude)
- `target/` - Rust build artifacts
- `.claude/` - Claude Code settings and rules
- `mp3rgui/` - GUI subproject (separate package)
- `packages/` - Package manager manifests (winget, etc.)
- `docs/` - Documentation
- `scripts/` - Build/release scripts
- `.github/` - GitHub Actions workflows
- `tests/` - Test files and fixtures

## Release Workflow

### Pre-Release Checklist (MUST DO BEFORE TAGGING)

**Never create a release tag without completing these local verifications:**

1. **Version consistency check**
   ```bash
   # Verify all version numbers match
   grep '^version' Cargo.toml mp3rgui/Cargo.toml
   # Ensure version matches intended release (e.g., 1.3.1)
   ```

2. **Cargo.lock is committed**
   ```bash
   git status Cargo.lock
   # If modified, commit it BEFORE tagging
   ```

3. **crates.io package size check**
   ```bash
   cargo package --list --allow-dirty | wc -l
   # Should be ~10-15 files, NOT thousands
   # If too many files, check `exclude` in Cargo.toml
   ```

4. **Local build test**
   ```bash
   cargo build --release
   cargo test
   ```

5. **Clean git status**
   ```bash
   git status
   # All relevant changes must be committed before tagging
   ```

### Release Tag Creation

Only after ALL checks pass:
```bash
git tag v<version>
git push origin v<version>
```

### If Release Workflow Fails

1. **Do NOT immediately re-tag** - investigate the failure first
2. Read the full error log: `gh run view <run-id> --log-failed`
3. Fix the issue locally and verify with the checklist above
4. Then delete and recreate the tag:
   ```bash
   git tag -d v<version>
   git push origin --delete v<version>
   git tag v<version>
   git push origin v<version>
   ```

### Common Release Failures and Prevention

| Failure | Cause | Prevention |
|---------|-------|------------|
| crates.io version exists | Cargo.toml version not bumped | Check version BEFORE tagging |
| Cargo.lock dirty | Cargo.lock not committed | Always commit Cargo.lock |
| Payload too large | Missing `exclude` in Cargo.toml | Run `cargo package --list` locally |
| AV false positive | Aggressive optimization | Use `lto = "thin"`, `strip = "debuginfo"` |

### Cargo.toml Package Settings

Required settings to avoid crates.io issues:
```toml
[package]
# ... other fields ...
exclude = ["mp3rgui/", "target/", "packages/", "docs/", "scripts/", ".github/", ".claude/", "tests/"]

[profile.release]
lto = "thin"           # Not "true" - reduces AV false positives
codegen-units = 1
strip = "debuginfo"    # Not "true" - preserves symbols for AV compatibility
```

## Winget Package Submission

### Updating Winget Manifest After Release

1. **Wait for release workflow to complete successfully**
2. **Get new SHA256 checksums**
   ```bash
   curl -sL https://github.com/M-Igashi/mp3rgain/releases/download/v<version>/mp3rgain-v<version>-windows-x86_64.zip.sha256
   curl -sL https://github.com/M-Igashi/mp3rgain/releases/download/v<version>/mp3rgain-v<version>-windows-arm64.zip.sha256
   ```
3. **Update `packages/winget/*.yaml`** with new version and checksums
4. **Commit and push to mp3rgain repo**
5. **Update winget-pkgs PR**

### Winget PR Creation (via fork)

```bash
cd /tmp && rm -rf winget-pkgs
gh repo clone microsoft/winget-pkgs -- --depth 1
cd winget-pkgs
git checkout -b mp3rgain-<version>
mkdir -p manifests/m/M-Igashi/mp3rgain/<version>
cp /Users/masanarihigashi/Projects/mp3rgain/packages/winget/*.yaml manifests/m/M-Igashi/mp3rgain/<version>/
git add manifests/m/M-Igashi/mp3rgain/<version>/
git commit -m "New package: M-Igashi.mp3rgain version <version>"
git remote add fork https://github.com/M-Igashi/winget-pkgs.git
git push fork mp3rgain-<version>
gh pr create --repo microsoft/winget-pkgs --base master --head M-Igashi:mp3rgain-<version> \
  --title "New package: M-Igashi.mp3rgain version <version>" \
  --body "..."
```

### Manifest Notes

- SHA256 must be UPPERCASE in winget manifests
- `ReleaseDate` format: YYYY-MM-DD
- VCRedist dependency is NOT required (static CRT linking)

## Required GitHub Secrets

- `SCOOP_BUCKET_TOKEN` - scoop-bucket push access
- `HOMEBREW_TAP_TOKEN` - homebrew-tap push access
- `CARGO_REGISTRY_TOKEN` - crates.io API token

## Windows Build Configuration

Static CRT linking (no VCRUNTIME140.dll dependency):
```yaml
env:
  RUSTFLAGS: ${{ contains(matrix.target, 'windows') && '-C target-feature=+crt-static' || '' }}
```
