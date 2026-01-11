# Contributing to mp3rgain

Thank you for your interest in contributing to mp3rgain!

## Getting Started

### Prerequisites

- Rust 1.70 or later
- Git

### Building from Source

```bash
git clone https://github.com/M-Igashi/mp3rgain.git
cd mp3rgain
cargo build --release
```

### Running Tests

```bash
cargo test
```

## How to Contribute

### Reporting Bugs

- Use the [Bug Report](https://github.com/M-Igashi/mp3rgain/issues/new?template=bug_report.md) template
- Include your OS, architecture, and mp3rgain version
- Provide steps to reproduce the issue
- If possible, include a sample MP3 file

### Windows Compatibility Testing

We especially welcome Windows testing reports! Please use the [Windows Compatibility Report](https://github.com/M-Igashi/mp3rgain/issues/new?template=windows_compatibility.md) template to share your results.

### Suggesting Features

- Use the [Feature Request](https://github.com/M-Igashi/mp3rgain/issues/new?template=feature_request.md) template
- Explain the problem you're trying to solve
- Describe your proposed solution

### Submitting Pull Requests

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Run tests (`cargo test`)
5. Run clippy (`cargo clippy`)
6. Format code (`cargo fmt`)
7. Commit your changes (`git commit -m 'Add amazing feature'`)
8. Push to the branch (`git push origin feature/amazing-feature`)
9. Open a Pull Request

## Code Style

- Follow standard Rust conventions
- Run `cargo fmt` before committing
- Run `cargo clippy` and address warnings
- Add tests for new functionality
- Update documentation as needed

## Priority Areas

We're especially looking for help with:

1. **Windows compatibility** - Testing and fixing Windows-specific issues
2. **ReplayGain support** - Implementing track/album gain analysis
3. **Additional format support** - Extending to other audio formats
4. **Documentation** - Improving docs and examples

## Questions?

Feel free to open an issue or start a discussion if you have questions!
