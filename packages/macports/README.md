# MacPorts Packaging

Portfile for submission to [macports/macports-ports](https://github.com/macports/macports-ports).

## Files

- `audio/mp3rgain/Portfile` — port definition (PortGroup `github 1.0` + `cargo 1.0`)

## Local Verification (on macOS with MacPorts installed)

```sh
# Copy into a local clone of macports-ports and lint
git clone --depth 1 https://github.com/macports/macports-ports.git /tmp/macports-ports
mkdir -p /tmp/macports-ports/audio/mp3rgain
cp packages/macports/audio/mp3rgain/Portfile /tmp/macports-ports/audio/mp3rgain/

cd /tmp/macports-ports/audio/mp3rgain
port lint --nitpick

# Build / install / uninstall test
sudo port -v install mp3rgain
mp3rgain --help
sudo port uninstall mp3rgain
```

## Submission / Update Workflow

The `M-Igashi/macports-ports` fork only lives on GitHub. Every time we touch
the Portfile, clone upstream into `/tmp`, push a branch to the fork, open or
update the PR, then delete the temp clone.

```sh
# Fresh /tmp clone (fork is M-Igashi/macports-ports, already on github.com)
rm -rf /tmp/macports-ports
gh repo clone macports/macports-ports /tmp/macports-ports -- --depth 1
cd /tmp/macports-ports
git checkout -b audio/mp3rgain-new-port
git remote add fork https://github.com/M-Igashi/macports-ports.git

# Copy canonical Portfile from this repo into the temp clone
mkdir -p audio/mp3rgain
cp ~/Projects/mp3rgain/packages/macports/audio/mp3rgain/Portfile audio/mp3rgain/

git add audio/mp3rgain/Portfile
git commit -m "audio/mp3rgain: new port"
git push -u fork audio/mp3rgain-new-port

gh pr create --repo macports/macports-ports --base master \
  --head M-Igashi:audio/mp3rgain-new-port \
  --title "audio/mp3rgain: new port" \
  --body "Lossless MP3/AAC volume normalizer using ReplayGain. Modern Rust reimplementation of mp3gain. Provides the same lossless AAC/M4A global_gain rewrite functionality as the orphaned aacgain port."

# Clean up temp clone after PR is opened/updated
cd ~ && rm -rf /tmp/macports-ports /tmp/macports-ports-test /tmp/mp3rgain-*.tar.gz
```

## Updating the Portfile for a New Release

1. Bump `github.setup` version in the Portfile.
2. Recompute checksums:
   ```sh
   curl -sL https://github.com/M-Igashi/mp3rgain/archive/refs/tags/v<version>.tar.gz -o /tmp/mp3rgain.tgz
   shasum -a 256 /tmp/mp3rgain.tgz
   openssl dgst -rmd160 /tmp/mp3rgain.tgz
   wc -c /tmp/mp3rgain.tgz
   ```
3. Update the `cargo.crates` block from the new `Cargo.lock` (one line per dependency: `name version checksum`).
4. Re-run `port lint --nitpick` and a local install test.

## Trac Ticket for aacgain Deprecation (Step 2)

After this Portfile is merged, file at https://trac.macports.org/ :

- **Title:** `audio/aacgain: deprecate (orphaned, unmaintained upstream since 2010, multiple unpatched CVEs)`
- **Body points:**
  - aacgain bundles vulnerable mpglibDBL, faad2, and mp4v2 — see project [docs/security.md](../../docs/security.md)
  - upstream inactive since 2010 (the `aacgain` Portfile itself notes this)
  - Homebrew already deprecated `aacgain` in April 2023
  - `mp3rgain` is now in tree as a safe Rust replacement that performs the same lossless AAC/M4A `global_gain` rewrite
- Optional follow-up: PR adding `replaced_by mp3rgain` to `audio/aacgain/Portfile`.
