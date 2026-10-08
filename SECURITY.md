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
