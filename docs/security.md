# Security

mp3rgain is a complete rewrite of the original mp3gain in Rust, providing memory safety guarantees and eliminating entire classes of vulnerabilities.

## Security Vulnerabilities in mp3gain and aacgain

### mp3gain

The original mp3gain has had numerous security vulnerabilities over its history. Many have been patched in recent versions (1.6.x) by replacing the bundled mpglibDBL with proper linking to the system libmpg123 library.

| CVE | CVSS | Type | mp3gain 1.6.2 |
|-----|------|------|---------------|
| CVE-2021-34085 | 9.8 (Critical) | Out-of-bounds Read in `III_dequantize_sample` | **Fixed** |
| CVE-2019-18359 | 5.5 (Medium) | Buffer over-read in `ReadMP3APETag` | **Fixed** (distro patches) |
| CVE-2017-9872 | - | Buffer over-read in `III_dequantize_sample` | **Fixed** |
| CVE-2017-14409 | 7.8 (High) | Buffer over-read in `III_i_stereo` | **Fixed** |
| CVE-2018-10778 | - | Heap-based buffer over-read in `II_step_one` | **Fixed** |
| CVE-2023-49356 | 7.5 (High) | Stack buffer overflow in `WriteMP3GainAPETag` | **Unpatched** |

CVE-2023-49356 was discovered in December 2023 and affects mp3gain v1.6.2. It allows denial of service via specially crafted files.

### aacgain

[aacgain](https://github.com/dgilman/aacgain) is a fork of mp3gain that adds AAC support. Unlike mp3gain 1.6.x, **aacgain still bundles the vulnerable mpglibDBL library** and has not migrated to libmpg123.

| CVE | CVSS | Type | aacgain 2.0.0 |
|-----|------|------|---------------|
| CVE-2021-34085 | 9.8 (Critical) | Out-of-bounds Read in `III_dequantize_sample` | **Unpatched** |
| CVE-2017-9872 | - | Buffer over-read in `III_dequantize_sample` | **Unpatched** |
| CVE-2017-14409 | 7.8 (High) | Buffer over-read in `III_i_stereo` | **Unpatched** |
| CVE-2017-14411 | - | Stack buffer overflow in `copy_mp` | **Unpatched** |

The aacgain project bundles `mpglibDBL` (an outdated fork of mpglib from mpg123) and has not applied the security fixes that mp3gain 1.6.x received.

## Why mp3rgain Is Safe

### 1. Different Architecture

mp3rgain uses a fundamentally different approach:

| Operation | Original mp3gain | mp3rgain |
|-----------|------------------|----------|
| Gain adjustment | Full MP3 decode/encode via mpglib | Direct binary manipulation of `global_gain` field |
| ReplayGain analysis | mpglib/libmpg123 (C library) | symphonia (pure Rust) |
| APE tag handling | Custom C code (apetag.c) | Rust implementation |

The historical vulnerabilities in mp3gain existed in two places:
1. **mpglibDBL** - A bundled, vulnerable fork of mpg123 (fixed in 1.6.x by linking to system libmpg123)
2. **apetag.c** - Custom APE tag handling code (CVE-2023-49356 still affects this)

mp3rgain uses neither of these components.

### 2. Memory Safety

mp3rgain is written in Rust, which provides compile-time guarantees against:

- **Buffer overflows/over-reads**: Rust's bounds checking prevents out-of-bounds memory access
- **Use-after-free**: Rust's ownership system prevents dangling pointer access
- **Null pointer dereference**: Rust's `Option` type eliminates null pointers
- **Data races**: Rust's borrow checker prevents concurrent data access issues

### 3. Safe Audio Decoding

For ReplayGain analysis, mp3rgain uses [symphonia](https://github.com/pdeljanov/Symphonia), a pure Rust audio decoding library that:

- Is written entirely in safe Rust (no unsafe C bindings)
- Has its own security-focused design
- Is actively maintained and audited

### 4. Minimal Attack Surface

mp3rgain's gain adjustment operation doesn't decode audio at all. It directly reads and modifies the `global_gain` field in MP3 frame headers, which is a simple 8-bit value manipulation. This eliminates the complex parsing code where buffer overflows typically occur.

## Verification

You can verify mp3rgain's safety:

```bash
# Check for unsafe code usage
cargo geiger

# Run with address sanitizer (requires nightly)
RUSTFLAGS="-Z sanitizer=address" cargo +nightly test

# Fuzz testing
cargo fuzz run fuzz_target
```

## Reporting Security Issues

If you discover a security vulnerability in mp3rgain, please report it by:

1. Opening a [GitHub Security Advisory](https://github.com/M-Igashi/mp3rgain/security/advisories/new)
2. Or emailing the maintainer directly

Please do not open public issues for security vulnerabilities.

## References

- [CVE-2021-34085](https://nvd.nist.gov/vuln/detail/CVE-2021-34085) - Fixed in mp3gain 1.6.2, unpatched in aacgain
- [CVE-2019-18359](https://nvd.nist.gov/vuln/detail/CVE-2019-18359) - Fixed in mp3gain 1.6.2-2
- [CVE-2023-49356](https://nvd.nist.gov/vuln/detail/CVE-2023-49356) - Unpatched in mp3gain 1.6.2
- [Debian mp3gain Security Tracker](https://security-tracker.debian.org/tracker/source-package/mp3gain)
- [aacgain repository](https://github.com/dgilman/aacgain) - Contains bundled mpglibDBL
- [symphonia - Pure Rust audio decoding](https://github.com/pdeljanov/Symphonia)
- [Rust Memory Safety](https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html)
