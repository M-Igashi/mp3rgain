# Security

mp3rgain is a complete rewrite of the original mp3gain in Rust, providing memory safety guarantees and eliminating entire classes of vulnerabilities.

## Security Vulnerabilities in the Original mp3gain

The original mp3gain has **19 known CVEs** spanning two decades. The upstream project is **effectively abandoned** — the last source release was v1.6.2 (circa 2009–2010), and the SourceForge bug tracker shows no developer response to security reports. Distributions maintain their own patches to keep the tool usable.

### Root Causes

The vulnerabilities fall into two categories:

1. **mpglibDBL** — An outdated, bundled fork of mpg123's mpglib. The upstream mpg123 has received extensive security fixes over the years, but mp3gain's fork has not. In v1.6.x, the build system can optionally link to the system libmpg123, but the Windows build and many distribution packages still ship the vulnerable bundled code.

2. **APE tag handling (apetag.c)** — Custom C code for reading and writing ReplayGain APE tags, with multiple buffer overflow vulnerabilities.

### Complete CVE List

#### mpglib / mpglibDBL Vulnerabilities

| CVE | Year | CVSS | Type | Affected Function |
|-----|------|------|------|-------------------|
| CVE-2003-0577 | 2003 | — | DoS (invalid bitrate) | `common.c` |
| CVE-2004-0805 | 2004 | — | Buffer overflow | `layer2.c` |
| CVE-2004-0991 | 2004 | — | Buffer overflow (MPEG header) | mpglib |
| CVE-2006-1655 | 2006 | — | Heap overflow (multiple) | `layer3.c` |
| CVE-2017-12912 | 2017 | — | Buffer over-read | `layer3.c` |
| CVE-2017-14406 | 2017 | — | NULL pointer dereference | `interface.c` (`sync_buffer`) |
| CVE-2017-14407 | 2017 | — | Stack buffer over-read | `gain_analysis.c` (`filterYule`) |
| CVE-2017-14408 | 2017 | — | Buffer over-read | `layer3.c` (`III_i_stereo`) |
| CVE-2017-14409 | 2017 | 7.8 (High) | Buffer overflow | `layer3.c` (`dct36` / `III_dequantize_sample`) |
| CVE-2017-14410 | 2017 | — | Read access violation | `layer3.c` (`III_dequantize_sample`) |
| CVE-2017-14411 | 2017 | — | Stack buffer overflow | `interface.c` (`copy_mp`) |
| CVE-2017-14412 | 2017 | — | Invalid memory write | `interface.c` |
| CVE-2017-9872 | 2017 | — | Buffer over-read | `layer3.c` (`III_dequantize_sample`) |
| CVE-2018-10776 | 2018 | — | Segfault / DoS | `common.c` (`getbits`) |
| CVE-2018-10778 | 2018 | — | Read access violation | `layer3.c` (`III_dequantize_sample`) |
| CVE-2020-15359 | 2020 | — | Stack overflow (code execution) | Local variable overflow |
| CVE-2021-34085 | 2021 | 9.8 (Critical) | Out-of-bounds read / memory corruption | `layer3.c` (`III_dequantize_sample`) |

#### APE Tag Vulnerabilities

| CVE | Year | CVSS | Type | Affected Function |
|-----|------|------|------|-------------------|
| CVE-2017-12911 | 2017 | — | Stack memory corruption | `apetag.c` |
| CVE-2018-10777 | 2018 | — | Buffer overflow | `apetag.c` (`WriteMP3GainAPETag`) |
| CVE-2019-18359 | 2019 | 5.5 (Medium) | Buffer over-read | `apetag.c` (`ReadMP3APETag`) |
| CVE-2023-49356 | 2023 | 7.5 (High) | Stack buffer overflow | `apetag.c` (`WriteMP3GainAPETag`) |

### Upstream Status

- **Last release**: v1.6.2 (source only, circa 2009–2010)
- **Website last updated**: September 2018 (translation update only)
- **Bug tracker**: Multiple unresolved security reports (SourceForge bugs #36, #47, #50)
- **Status**: Effectively abandoned — no developer response to CVE reports

CVE-2020-15359 was discovered by VDA Labs using the ForAllSecure Mayhem fuzzer, which found ~1,600 crashes out of ~6,000 test cases, including pointer overwrites that could allow code execution hijacking. The developer was contacted but no fix was released.

## Distribution Patch Status

Since upstream is abandoned, distributions maintain their own patches independently.

### Debian / Ubuntu

Debian is the most actively maintained distribution package.

- **Current version**: 1.6.2-3
- **Patches applied**: `fix-security-bugs.patch` (covers CVE-2019-18359, CVE-2023-49356, and other APE tag issues)
- **Security tracker**: [Debian mp3gain tracker](https://security-tracker.debian.org/tracker/source-package/mp3gain)
- **History**: mp3gain was **removed from Debian in 2014** due to security issues and lack of maintainer, then re-introduced in 2018 with patches

Ubuntu inherits Debian's patches.

### openSUSE

- Issued security updates in 2020:
  - **openSUSE-SU-2020:0522-1**: Fixed CVE-2017-12911, CVE-2019-18359
  - **openSUSE-SU-2020:0539-1**: Same fixes for Backports SLE-15-SP1

### Fedora

- Package exists for mp3gain 1.6.2
- CVE-2018-10777 tracked in Red Hat Bugzilla (#1903788, #1903789, #1903790)

### Arch Linux

- **Removed from official repositories**
- Available only in AUR (user-maintained) — **security patches likely not applied**

### Other Distributions

- **Mageia**: Bug #21706 tracks CVE-2017-14406 through CVE-2017-14412
- **Gentoo**: Agostino Sarubbo originally discovered many of the 2017 CVEs via fuzzing

### Summary

| Distribution | Patched? | Notes |
|-------------|----------|-------|
| Debian/Ubuntu | Yes | Most comprehensive patches |
| openSUSE | Partial | 2020 security updates for select CVEs |
| Fedora | Partial | Some CVEs tracked in Bugzilla |
| Arch Linux | No | AUR only, no official support |
| Upstream | No | Abandoned, no fixes released |

## aacgain

[aacgain](https://github.com/dgilman/aacgain) is a fork of mp3gain that adds AAC support. The upstream project is **effectively abandoned** — the last release was v2.0.0 (December 2019), with a final commit in July 2022. No security fixes have been applied.

### Bundled Vulnerable Libraries

aacgain bundles **all dependencies as source code**, creating a large attack surface with no security updates:

#### mpglibDBL (mp3gain/mpglibDBL/)

Unlike mp3gain 1.6.x which migrated to system libmpg123, aacgain still bundles the vulnerable mpglibDBL fork. **All 17 mpglibDBL-derived CVEs from the mp3gain table above are unpatched in aacgain.**

#### faad2 (3rdparty/faad2/)

aacgain bundles a fork of faad2 (updated to ~2.9.1 in July 2022). faad2 itself has known vulnerabilities:

- CVE-2008-4201 — Heap-based buffer overflow
- [Gentoo GLSA 202006-17](https://security.gentoo.org/glsa/202006-17) — Multiple vulnerabilities
- [Gentoo GLSA 202401-13](https://security.gentoo.org/glsa/202401-13) — Additional vulnerabilities

#### mp4v2 (3rdparty/mp4v2/)

aacgain bundles mp4v2 3.0.4. mp4v2 has multiple CVEs:

| CVE | Type |
|-----|------|
| CVE-2018-14326 | Integer overflow |
| CVE-2018-14379 | Memory corruption |
| CVE-2023-1451 | Denial of service |
| CVE-2023-29584 | Heap buffer overflow |

#### APE tag / gain analysis code

The same vulnerable `apetag.c` and `gain_analysis.c` from mp3gain are included without patches:

| CVE | CVSS | Type |
|-----|------|------|
| CVE-2023-49356 | 7.5 (High) | Stack buffer overflow in `WriteMP3GainAPETag` |
| CVE-2019-18359 | 5.5 (Medium) | Buffer over-read in `ReadMP3APETag` |
| CVE-2018-10777 | — | Buffer overflow in `WriteMP3GainAPETag` |
| CVE-2017-12911 | — | Stack memory corruption in `apetag.c` |
| CVE-2017-14407 | — | Stack buffer over-read in `filterYule` |

### aacgain Distribution Status

No distribution applies security patches to aacgain. All ship the unpatched upstream code.

| Channel | Version | Patched? | Notes |
|---------|---------|----------|-------|
| Homebrew | 1.8 | No | **Deprecated as unmaintained** (April 2023) |
| MacPorts | 1.8 | No | Cannot update — v1.9 tarball missing |
| Chocolatey | 1.9.0.2 | No | |
| AUR (aacgain-cvs) | 20130814 | No | Maintainer notes "really bad code" |
| Deb Multimedia | 2.0.0 | No | Unofficial repository |
| NixOS | 2.0.0-unstable | No | Git snapshot |

**Not available in**: Debian/Ubuntu official repos, Fedora/RHEL, Scoop

## mp3gain Distribution Channels

In addition to the Linux distribution packages described above, mp3gain is available through several other channels — most without security patches.

| Channel | Version | Patched? | Notes |
|---------|---------|----------|-------|
| Debian/Ubuntu | 1.6.2-3 | **Yes** | Most comprehensive patches, links to system libmpg123 |
| openSUSE | 1.6.2 | Partial | Security updates issued in 2020 |
| Fedora | 1.6.2 | Partial | Some CVEs tracked |
| Homebrew | 1.6.2 | **No** | Builds from source, links to libmpg123 but no APE tag patches |
| Arch Linux (AUR) | 1.6.2 | **No** | User-maintained, no official support |
| Chocolatey | **1.5.2** | **No** | **Not updated since 2014 — includes mpglibDBL, extremely dangerous** |
| SourceForge (Windows) | **1.2.5** (stable) | **No** | **2004-era binary — contains CVE-2003 through CVE-2006 vulnerabilities** |

### Notable Risks

- **Chocolatey mp3gain 1.5.2**: Ships the pre-1.6.x version that still bundles mpglibDBL. All 19 CVEs apply.
- **SourceForge Windows binary**: The "stable" download (v1.2.5) dates from ~2004 and is vulnerable to every known CVE.
- **Homebrew mp3gain 1.6.2**: Links to system libmpg123 (mitigating mpglibDBL CVEs), but does **not** apply Debian's APE tag security patches. CVE-2023-49356 and CVE-2019-18359 remain unpatched.

## Why mp3rgain Is Safe

### 1. Different Architecture

mp3rgain uses a fundamentally different approach:

| Operation | Original mp3gain | mp3rgain |
|-----------|------------------|----------|
| Gain adjustment | Full MP3 decode/encode via mpglib | Direct binary manipulation of `global_gain` field |
| ReplayGain analysis | mpglib/libmpg123 (C library) | symphonia (pure Rust) |
| APE tag handling | Custom C code (apetag.c) | Rust implementation |

mp3rgain uses **none** of the vulnerable components from the original mp3gain.

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

### CVE Databases
- [NVD - CVE-2021-34085](https://nvd.nist.gov/vuln/detail/CVE-2021-34085)
- [NVD - CVE-2023-49356](https://nvd.nist.gov/vuln/detail/CVE-2023-49356)
- [NVD - CVE-2019-18359](https://nvd.nist.gov/vuln/detail/CVE-2019-18359)
- [NVD - CVE-2017-14409](https://nvd.nist.gov/vuln/detail/CVE-2017-14409)
- [NVD - CVE-2020-15359](https://nvd.nist.gov/vuln/detail/CVE-2020-15359)

### Distribution Security Trackers
- [Debian mp3gain Security Tracker](https://security-tracker.debian.org/tracker/source-package/mp3gain)
- [Debian mp3gain Patches](https://sources.debian.org/patches/mp3gain/1.6.2-3/)
- [Ubuntu CVE Tracker - mp3gain](https://ubuntu.com/security/cves?q=mp3gain)

### Vulnerability Reports
- [CVE-2020-15359 - VDA Labs / Mayhem fuzzer discovery](https://www.mayhem.security/blog/cve-2020-15359-vdalabs-uses-mayhem-to-find-mp3gain-stack-overflow)
- [CVE-2023-49356 - Stack buffer overflow report](https://github.com/linzc21/bug-reports/blob/main/reports/mp3gain/1.6.2/stack-buffer-overflow/CVE-2023-49356.md)
- [SourceForge Bug #36 - Arbitrary code execution](https://sourceforge.net/p/mp3gain/bugs/36/)
- [Gentoo - mp3gain buffer overflow discovery](https://blogs.gentoo.org/ago/2017/09/08/mp3gain-global-buffer-overflow-in-iii_dequantize_sample-mpglibdbllayer3-c/)
- [oss-security mailing list - mp3gain](https://www.openwall.com/lists/oss-security/2017/09/14/3)

### Related Projects
- [aacgain repository](https://github.com/dgilman/aacgain) — Contains bundled mpglibDBL, faad2, mp4v2
- [faad2 security advisories (Gentoo GLSA 202006-17)](https://security.gentoo.org/glsa/202006-17)
- [faad2 security advisories (Gentoo GLSA 202401-13)](https://security.gentoo.org/glsa/202401-13)
- [mp4v2 CVEs](https://www.cvedetails.com/product/44070/Mp4v2-Project-Mp4v2.html)
- [symphonia - Pure Rust audio decoding](https://github.com/pdeljanov/Symphonia)
- [Rust Memory Safety](https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html)

### Package Repositories
- [mp3gain on Repology](https://repology.org/project/mp3gain/versions)
- [aacgain on Repology](https://repology.org/project/aacgain/versions)
- [Homebrew mp3gain formula](https://formulae.brew.sh/formula/mp3gain)
- [Chocolatey mp3gain](https://community.chocolatey.org/packages/mp3gain)
