# GitHub Releases

GitHub Releases is the primary binary distribution channel. Pushing a tag that starts with `v` triggers `.github/workflows/release.yml`, which verifies the source, builds and packages every target, and publishes the release.

## Distribution targets

| OS | Architecture | Rust target | Archive |
| --- | --- | --- | --- |
| macOS | Intel | `x86_64-apple-darwin` | `.tar.gz` |
| macOS | Apple Silicon | `aarch64-apple-darwin` | `.tar.gz` |
| Linux | x86_64 | `x86_64-unknown-linux-musl` | `.tar.gz` |
| Linux | arm64 | `aarch64-unknown-linux-musl` | `.tar.gz` |

Windows is not distributed yet because executable resolution and ownership detection are not implemented for Windows. Linux artifacts use statically linked musl targets to avoid a dependency on the host's glibc version.

Each archive contains:

- `whowns`
- `install.sh`
- `README.md`
- `README.ja.md`
- `LICENSE`

## Release procedure

1. Update the version in `Cargo.toml`.
2. Run `cargo check` to synchronize the package version in `Cargo.lock`.
3. Run `cargo fmt --all -- --check`, `cargo test --locked --all-targets`, and `cargo clippy --locked --all-targets -- -D warnings`.
4. Commit the version change.
5. Create and push an annotated tag that matches the Cargo package version.

```sh
git tag -a v0.1.0 -m "v0.1.0"
git push origin v0.1.0
```

The workflow stops if the tag and `Cargo.toml` versions do not match. A release is published with GitHub-generated release notes only after every target succeeds. A version containing `-` is published as a prerelease.

## User verification

Download the target archive and `SHA256SUMS` from the release.

Linux:

```sh
sha256sum --check SHA256SUMS --ignore-missing
```

macOS:

```sh
grep 'whowns-v0.1.0-aarch64-apple-darwin.tar.gz' SHA256SUMS \
  | shasum --algorithm 256 --check
```

Public repositories also produce GitHub artifact attestations.

```sh
gh attestation verify whowns-v0.1.0-aarch64-apple-darwin.tar.gz \
  --repo shm11C3/whowns
```

## Installation

Extract the archive and run the bundled installer.

```sh
tar -xzf whowns-v0.1.0-aarch64-apple-darwin.tar.gz
cd whowns-v0.1.0-aarch64-apple-darwin
./install.sh
```

The default destination is `$HOME/.local/bin/whowns`.

```sh
./install.sh --bin-dir /usr/local/bin
```

## Repository settings before publication

- Allow GitHub Actions workflows to create releases.
- Keep the repository public to use artifact attestations on supported GitHub plans.
- Enable immutable releases.
- Until Developer ID signing and notarization are implemented, state in the release notes that the macOS archives are unsigned.
