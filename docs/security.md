# Security

mp3rgain is a rewrite of mp3gain in Rust. It shares no code with mp3gain or aacgain, and none of the C components behind their published CVEs. This page lists those CVEs, where they live, and why their bug classes do not carry over. It describes the design rather than a tested guarantee: no CVE proof-of-concept file is part of mp3rgain's test suite, and there is no fuzzing harness yet.

To report a vulnerability in mp3rgain, see [SECURITY.md](../SECURITY.md).

## Vulnerabilities in the original mp3gain

The tables below list 21 CVEs in mp3gain and the code it bundles, spanning two decades. The last source release was 1.6.2 (circa 2009-2010). In November 2025 the maintainer applied the Debian/openSUSE security patches to the upstream master branch, fixing the APE tag vulnerabilities. No new release has been made since, the Windows binaries remain unpatched, and the mpglibDBL CVEs are only avoided when building from source against the system libmpg123.

### Root causes

Most of the vulnerabilities fall into two groups:

1. **mpglibDBL**: an outdated, bundled fork of mpg123's mpglib decoder. Upstream mpg123 has received extensive security fixes over the years; mp3gain's fork has not. Since 1.6.x the build can link the system libmpg123 instead, but the Windows build and many distribution packages still ship the bundled code.
2. **APE tag handling (`apetag.c`)**: custom C code for reading and writing ReplayGain APE tags, with several buffer overflows.

### Decoder and analysis CVEs

| CVE | Year | CVSS | Type | Affected function |
|-----|------|------|------|-------------------|
| CVE-2003-0577 | 2003 | | DoS (invalid bitrate) | `common.c` |
| CVE-2004-0805 | 2004 | | Buffer overflow | `layer2.c` |
| CVE-2004-0991 | 2004 | | Buffer overflow (MPEG header) | mpglib |
| CVE-2006-1655 | 2006 | | Heap overflow (multiple) | `layer3.c` |
| CVE-2017-12912 | 2017 | | Buffer over-read | `layer3.c` |
| CVE-2017-14406 | 2017 | | NULL pointer dereference | `interface.c` (`sync_buffer`) |
| CVE-2017-14407 | 2017 | | Stack buffer over-read | `gain_analysis.c` (`filterYule`) |
| CVE-2017-14408 | 2017 | | Buffer over-read | `layer3.c` (`III_i_stereo`) |
| CVE-2017-14409 | 2017 | 7.8 (High) | Buffer overflow | `layer3.c` (`dct36` / `III_dequantize_sample`) |
| CVE-2017-14410 | 2017 | | Read access violation | `layer3.c` (`III_dequantize_sample`) |
| CVE-2017-14411 | 2017 | | Stack buffer overflow | `interface.c` (`copy_mp`) |
| CVE-2017-14412 | 2017 | | Invalid memory write | `interface.c` |
| CVE-2017-9872 | 2017 | | Buffer over-read | `layer3.c` (`III_dequantize_sample`) |
| CVE-2018-10776 | 2018 | | Segfault / DoS | `common.c` (`getbits`) |
| CVE-2018-10778 | 2018 | | Read access violation | `layer3.c` (`III_dequantize_sample`) |
| CVE-2020-15359 | 2020 | | Stack overflow (code execution) | Local variable overflow |
| CVE-2021-34085 | 2021 | 9.8 (Critical) | Out-of-bounds read / memory corruption | `layer3.c` (`III_dequantize_sample`) |

CVE-2020-15359 was found by VDA Labs with the ForAllSecure Mayhem fuzzer, which reported about 1,600 crashes out of about 6,000 test cases, including pointer overwrites that could allow code execution.

### APE tag CVEs

These were fixed in upstream master on 2025-11-01 (commit `b0d6a5`) by applying the Debian/openSUSE patches. Without a new release, pre-built binaries and distributions that package releases rather than git HEAD remain affected.

| CVE | Year | CVSS | Type | Affected function | Upstream master |
|-----|------|------|------|-------------------|-----------------|
| CVE-2017-12911 | 2017 | | Stack memory corruption | `apetag.c` | Fixed |
| CVE-2018-10777 | 2018 | | Buffer overflow | `apetag.c` (`WriteMP3GainAPETag`) | Fixed |
| CVE-2019-18359 | 2019 | 5.5 (Medium) | Buffer over-read | `apetag.c` (`ReadMP3APETag`) | Fixed |
| CVE-2023-49356 | 2023 | 7.5 (High) | Stack buffer overflow | `apetag.c` (`WriteMP3GainAPETag`) | Fixed |

SourceForge bugs #56-#60 (heap-buffer-overflow in `ReadMP3APETag`, reported July 2025) may also be addressed by those patches, but this has not been confirmed against the specific PoC files.

### Upstream status

- **Last release:** 1.6.2 (source only, circa 2009-2010)
- **Latest commit:** 2025-11-01, the Debian/openSUSE APE tag patches (`b0d6a5`)
- **Website last updated:** September 2018 (translation update only)
- **Bug tracker:** SourceForge bugs #56-#60 (July 2025, heap-buffer-overflow) remain open
- **Status:** minimally maintained. The developer (Glen Sawyer) responded to bug #62 and applied the patches in November 2025, but has not made a release.

The Windows binaries on the SourceForge download page (1.2.5 stable, 1.3.4 beta) predate every fix and are affected by every CVE listed above.

## mp3gain distribution status

Because there is no patched release, each distribution decides for itself. As of November 2025:

| Channel | Version | Patched? | Notes |
|---------|---------|----------|-------|
| Debian/Ubuntu | 1.6.2-3 | **Yes** | Most comprehensive patches; links the system libmpg123 |
| openSUSE | 1.6.2 | Partial | Security updates issued in 2020 |
| Fedora | 1.6.2 | Partial | Some CVEs tracked in Bugzilla |
| Homebrew | 1.6.2 | **No** | Builds the release source against libmpg123, but without the APE tag patches |
| Arch Linux (AUR) | 1.6.2 | **No** | Removed from the official repositories; user-maintained |
| Chocolatey | **1.5.2** | **No** | Not updated since 2014; bundles mpglibDBL, so every CVE above applies |
| SourceForge (Windows) | **1.2.5** (stable) | **No** | 2004-era binary, affected by every CVE above |

Details:

- **Debian / Ubuntu:** `fix-security-bugs.patch` covers CVE-2019-18359, CVE-2023-49356 and other APE tag issues ([security tracker](https://security-tracker.debian.org/tracker/source-package/mp3gain)). mp3gain was removed from Debian in 2014 over security issues and the lack of a maintainer, and re-introduced in 2018 with patches. Ubuntu inherits Debian's patches.
- **openSUSE:** openSUSE-SU-2020:0522-1 fixed CVE-2017-12911 and CVE-2019-18359; openSUSE-SU-2020:0539-1 shipped the same fixes for Backports SLE-15-SP1.
- **Fedora:** CVE-2018-10777 is tracked in Red Hat Bugzilla (#1903788, #1903789, #1903790).
- **Homebrew:** linking libmpg123 avoids the mpglibDBL CVEs, but the APE tag fixes exist only in upstream master, which Homebrew does not build.
- **Mageia:** bug #21706 tracks CVE-2017-14406 through CVE-2017-14412.
- **Gentoo:** Agostino Sarubbo found many of the 2017 CVEs by fuzzing.

## aacgain

[aacgain](https://github.com/dgilman/aacgain) is a fork of mp3gain that adds AAC support. Its last release is 2.0.0 (December 2019) and its last commit was in July 2022. No security fixes have been applied.

aacgain bundles all of its dependencies as source code, none of them updated for security:

- **mpglibDBL** (`mp3gain/mpglibDBL/`): unlike mp3gain 1.6.x, which can link the system libmpg123, aacgain still bundles the mpglibDBL fork, so the mpglibDBL CVEs in the first table above are unpatched.
- **faad2** (`3rdparty/faad2/`, updated to about 2.9.1 in July 2022): faad2 has its own vulnerabilities, including CVE-2008-4201 (heap-based buffer overflow), [Gentoo GLSA 202006-17](https://security.gentoo.org/glsa/202006-17) and [Gentoo GLSA 202401-13](https://security.gentoo.org/glsa/202401-13).
- **mp4v2** (`3rdparty/mp4v2/`, 3.0.4): CVE-2018-14326 (integer overflow), CVE-2018-14379 (memory corruption), CVE-2023-1451 (denial of service), CVE-2023-29584 (heap buffer overflow).
- **`apetag.c` and `gain_analysis.c`** from mp3gain, without the patches: CVE-2023-49356, CVE-2019-18359, CVE-2018-10777, CVE-2017-12911 and CVE-2017-14407.

No distribution patches aacgain:

| Channel | Version | Patched? | Notes |
|---------|---------|----------|-------|
| Homebrew | 1.8 | No | Deprecated as unmaintained (April 2023) |
| MacPorts | 1.8 | No | Cannot update: the 1.9 tarball is missing |
| Chocolatey | 1.9.0.2 | No | |
| AUR (aacgain-cvs) | 20130814 | No | Maintainer notes "really bad code" |
| Deb Multimedia | 2.0.0 | No | Unofficial repository |
| NixOS | 2.0.0-unstable | No | Git snapshot |

aacgain is not available in the official Debian/Ubuntu or Fedora/RHEL repositories, or in Scoop.

## Why these CVEs do not carry over to mp3rgain

### None of the vulnerable code is used

| Component | mp3gain / aacgain | mp3rgain |
|-----------|-------------------|----------|
| Gain adjustment | Rewrites `global_gain` in the bitstream, no re-encode (C) | The same approach (Rust) |
| Decoding for analysis | mpglib / mpglibDBL or libmpg123; faad2 for AAC (C) | [symphonia](https://github.com/pdeljanov/Symphonia) (Rust) |
| MP4 container | mp4v2 (C) | symphonia, plus mp3rgain's own MP4 parser (Rust) |
| APE tags | `apetag.c` (C) | `src/ape.rs` (Rust) |

mp3rgain does not contain mpglib, mpglibDBL, faad2, mp4v2, `apetag.c` or `gain_analysis.c`; its ReplayGain analysis is a Rust implementation checked against the reference `gain_analysis.c` output.

### Safe Rust

mp3rgain's own source (the library, the CLI and the GUI) contains no `unsafe` blocks. This is a property of the current code that you can check with `grep`; the crate does not declare `#![forbid(unsafe_code)]`, so the compiler does not enforce it. In safe Rust:

- **Out-of-bounds access** cannot corrupt memory: every index is bounds-checked at run time, and a bad one stops the program with a panic instead.
- **Use-after-free** and dangling pointers are rejected at compile time by ownership and borrowing.
- **Null pointer dereferences** cannot happen: absence is an explicit `Option`.
- **Data races** are rejected at compile time.

The buffer overflows, over-reads and invalid writes that make up nearly every CVE above are exactly the class this removes. What safe Rust does not remove is ordinary bugs: a malformed file can still make mp3rgain fail with an error or spend more memory than it should. The parsers check sizes read from a file against the data actually present (for example APE tag sizes and MP4 sample tables), and such a failure is worth [reporting](../SECURITY.md).

The symphonia crates mp3rgain uses for decoding contain no `unsafe` code either, and most declare `#![forbid(unsafe_code)]`. Some other dependencies, such as rayon for parallelism, do use `unsafe` internally.

### What a gain change touches

Applying a gain to an MP3 does not decode any audio: mp3rgain finds each frame, reads its side information and changes the 8-bit `global_gain` field of each granule and channel. AAC needs more parsing, because finding every `global_gain` means walking the bitstream element by element (`src/aac.rs`); that parser is safe Rust too. Decoding with symphonia happens only for analysis (`-r`, `-a`, `-x` and the default scan).

## Checking it yourself

```bash
# No unsafe blocks in mp3rgain's own code (prints nothing)
grep -rnw unsafe src mp3rgui/src

# Known-vulnerable dependencies (CI runs this for both lockfiles)
cargo audit
cargo audit --file mp3rgui/Cargo.lock

# Unsafe usage including dependencies (third-party tool: cargo install cargo-geiger)
cargo geiger
```

CI also runs CodeQL and GitHub's dependency review; see [SECURITY.md](../SECURITY.md).

## References

### CVE databases

- [NVD - CVE-2021-34085](https://nvd.nist.gov/vuln/detail/CVE-2021-34085)
- [NVD - CVE-2023-49356](https://nvd.nist.gov/vuln/detail/CVE-2023-49356)
- [NVD - CVE-2019-18359](https://nvd.nist.gov/vuln/detail/CVE-2019-18359)
- [NVD - CVE-2017-14409](https://nvd.nist.gov/vuln/detail/CVE-2017-14409)
- [NVD - CVE-2020-15359](https://nvd.nist.gov/vuln/detail/CVE-2020-15359)

### Distribution security trackers

- [Debian mp3gain Security Tracker](https://security-tracker.debian.org/tracker/source-package/mp3gain)
- [Debian mp3gain Patches](https://sources.debian.org/patches/mp3gain/1.6.2-3/)
- [Ubuntu CVE Tracker - mp3gain](https://ubuntu.com/security/cves?q=mp3gain)

### Vulnerability reports

- [CVE-2020-15359 - VDA Labs / Mayhem fuzzer discovery](https://www.mayhem.security/blog/cve-2020-15359-vdalabs-uses-mayhem-to-find-mp3gain-stack-overflow)
- [CVE-2023-49356 - Stack buffer overflow report](https://github.com/linzc21/bug-reports/blob/main/reports/mp3gain/1.6.2/stack-buffer-overflow/CVE-2023-49356.md)
- [SourceForge Bug #36 - Arbitrary code execution](https://sourceforge.net/p/mp3gain/bugs/36/) (fixed via libmpg123 migration)
- [SourceForge Bug #62 - Backport Debian/openSUSE patches](https://sourceforge.net/p/mp3gain/bugs/62/) (applied 2025-11-01)
- [Gentoo - mp3gain buffer overflow discovery](https://blogs.gentoo.org/ago/2017/09/08/mp3gain-global-buffer-overflow-in-iii_dequantize_sample-mpglibdbllayer3-c/)
- [oss-security mailing list - mp3gain](https://www.openwall.com/lists/oss-security/2017/09/14/3)

### Related projects

- [aacgain repository](https://github.com/dgilman/aacgain): contains bundled mpglibDBL, faad2, mp4v2
- [faad2 security advisories (Gentoo GLSA 202006-17)](https://security.gentoo.org/glsa/202006-17)
- [faad2 security advisories (Gentoo GLSA 202401-13)](https://security.gentoo.org/glsa/202401-13)
- [mp4v2 CVEs](https://www.cvedetails.com/product/44070/Mp4v2-Project-Mp4v2.html)
- [symphonia - Pure Rust audio decoding](https://github.com/pdeljanov/Symphonia)
- [Rust Memory Safety](https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html)

### Package repositories

- [mp3gain on Repology](https://repology.org/project/mp3gain/versions)
- [aacgain on Repology](https://repology.org/project/aacgain/versions)
- [Homebrew mp3gain formula](https://formulae.brew.sh/formula/mp3gain)
- [Chocolatey mp3gain](https://community.chocolatey.org/packages/mp3gain)
