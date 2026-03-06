# mp3rgain - Project Guidelines

Lossless MP3 volume normalizer using ReplayGain. A modern Rust reimplementation of the classic mp3gain tool.

## Tech Stack

- **Language**: Rust
- **Distribution**: crates.io, Homebrew, Winget
- **Subproject**: mp3rgui/ (GUI version, separate package)

## Project-Specific Rules

See `.claude/rules/` for detailed guidelines:

| Rule File | Contents |
|-----------|----------|
| [release.md](.claude/rules/release.md) | Pre-release checklist, Cargo.toml settings, common failures |
| [winget.md](.claude/rules/winget.md) | Winget manifest submission workflow |
| [internal-files.md](.claude/rules/internal-files.md) | Files excluded from git and crates.io |
| [formatting.md](.claude/rules/formatting.md) | Code formatting rules (`cargo fmt` before commit) |

## Quick Reference

### Build Commands

```bash
cargo build --release
cargo test
cargo package --list --allow-dirty  # Check crates.io contents
```

### Release Tag

```bash
# Only after pre-release checklist passes
git tag v<version>
git push origin v<version>
```

### Related Projects

- **homebrew-tap** - Homebrew formula
- **winget-pkgs** - Winget manifest (M-Igashi fork)
