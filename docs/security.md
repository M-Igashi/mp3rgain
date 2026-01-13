# Security

mp3rgain is a complete rewrite of the original mp3gain in Rust, eliminating known security vulnerabilities present in the C implementation.

## Known Vulnerabilities in Original mp3gain

The original mp3gain has several unpatched security vulnerabilities:

### CVE-2021-34085 (Critical)

- **CVSS Score**: 9.8 (Critical)
- **Type**: Out-of-bounds Read (CWE-125)
- **Location**: `III_dequantize_sample` function in `mpglibDBL/layer3.c`
- **Impact**: Remote code execution, application crash
- **Details**: A read access violation allows remote attackers to crash the application or potentially execute arbitrary code through a malformed MP3 file.

### CVE-2019-18359 (Medium)

- **CVSS Score**: 5.5 (Medium)
- **Type**: Out-of-bounds Read (CWE-125)
- **Location**: `ReadMP3APETag` function in `apetag.c`
- **Impact**: Denial of service (application crash)
- **Details**: A heap-based buffer over-read in APE tag parsing causes crashes when processing malformed files.

### Other Known CVEs

- **CVE-2017-9872**: Buffer over-read in `III_dequantize_sample`
- **CVE-2017-14409**: Buffer over-read in `III_i_stereo`
- **CVE-2018-10778**: Heap-based buffer over-read in `II_step_one`

## Why mp3rgain Is Not Affected

### 1. Different Architecture

mp3rgain uses a fundamentally different approach:

| Operation | Original mp3gain | mp3rgain |
|-----------|------------------|----------|
| Gain adjustment | Full MP3 decode/encode via mpglib | Direct binary manipulation of `global_gain` field |
| ReplayGain analysis | mpglib (C library) | symphonia (pure Rust) |
| APE tag handling | Custom C code | Rust implementation |

The vulnerabilities in the original mp3gain exist in the `mpglibDBL` library, which mp3rgain does not use at all.

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

- [CVE-2021-34085](https://nvd.nist.gov/vuln/detail/CVE-2021-34085)
- [CVE-2019-18359](https://nvd.nist.gov/vuln/detail/CVE-2019-18359)
- [symphonia - Pure Rust audio decoding](https://github.com/pdeljanov/Symphonia)
- [Rust Memory Safety](https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html)
