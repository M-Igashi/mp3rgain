# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 3.x     | :white_check_mark: |
| < 3.0   | :x:                |

## Reporting a Vulnerability

If you discover a security vulnerability in mp3rgain, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please use [GitHub's private vulnerability reporting](https://github.com/M-Igashi/mp3rgain/security/advisories/new) to submit your report.

### What to include

- Description of the vulnerability
- Steps to reproduce
- Affected version(s)
- Potential impact

### Response timeline

- **Acknowledgment**: within 7 days
- **Fix or mitigation**: best effort, typically within 30 days

## Security Measures

The library, CLI and GUI source contains no `unsafe` blocks, and audio is decoded with [symphonia](https://github.com/pdeljanov/Symphonia) (pure Rust), not mpglib. See [docs/security.md](docs/security.md) for how this relates to the known mp3gain CVEs.

Automated checks:

- **cargo audit**: RustSec advisory scan of both the CLI and GUI lockfiles in CI on pushes and pull requests to `master`
- **Dependency review**: fails a pull request that adds a dependency with a known vulnerability of moderate severity or higher
- **Dependabot**: dependency updates and vulnerability alerts
- **CodeQL**: static analysis for Rust and GitHub Actions
- **Secret scanning** with push protection: blocks accidental credential commits

## Verifying Release Downloads

Since v3.16.0, every archive (`.tar.gz`, `.zip`), macOS disk image (`.dmg`), Windows installer (`.exe`), and Debian package (`.deb`) on [GitHub Releases](https://github.com/M-Igashi/mp3rgain/releases) carries a GitHub build provenance attestation. It shows that the file was built by this repository's `release.yml` from the tagged commit, which a `.sha256` file alone cannot: the checksum only shows that the download is intact. With the [GitHub CLI](https://cli.github.com/):

```bash
gh attestation verify mp3rgain-v3.16.1-linux-x86_64.tar.gz --repo M-Igashi/mp3rgain
```

Not attested: the `.sha256` files themselves, the apt repository index (signed with its own key instead; see the README), the crates.io package, and the Docker image.
