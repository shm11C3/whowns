# GitHub Releases

GitHub Releases is the primary binary distribution channel. Merging a package version change to `main` runs `.github/workflows/tag-release.yml`, which pushes the matching `v` tag and starts `.github/workflows/release.yml`. The release workflow verifies the source, builds and packages every target, and creates a draft release for review.

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

Each published release also contains these top-level assets:

- `install.sh` for `curl | sh` installation
- `SHA256SUMS` covering every archive and the installer

## Release procedure

1. Update the version in `Cargo.toml`.
2. Run `cargo check` to synchronize the package version in `Cargo.lock`.
3. Run `cargo fmt --all -- --check`, `cargo test --locked --all-targets`, `cargo clippy --locked --all-targets -- -D warnings`, and `sh tests/install.sh`.
4. Open and merge a pull request containing the version change.
5. Confirm that the `Tag release` workflow pushed the matching annotated tag and started the `Release` workflow.
6. Review the generated notes and assets in the draft release, then publish it manually.

The tag workflow stops if `Cargo.lock` is not synchronized or the tag already exists. It never moves an existing tag. The release workflow stops if the tag and `Cargo.toml` versions do not match. A draft release with GitHub-generated release notes is created only after every target succeeds. A version containing `-` is marked as a prerelease when published.

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

Install the latest release directly. The installer selects the target archive and verifies its SHA-256 checksum before writing the binary.

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/shm11C3/whowns/releases/latest/download/install.sh | sh
```

The default destination is `$HOME/.local/bin/whowns`. The `wio` shorthand alias is installed by default; pass `--no-alias` to omit it.

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/shm11C3/whowns/releases/latest/download/install.sh \
  | sh -s -- --bin-dir /usr/local/bin --no-alias
```

Alternatively, extract the archive and run its bundled installer.

```sh
tar -xzf whowns-v0.1.0-aarch64-apple-darwin.tar.gz
cd whowns-v0.1.0-aarch64-apple-darwin
./install.sh
```

## Repository settings before publication

- Allow GitHub Actions workflows to create tags and releases.
- Keep the repository public to use artifact attestations on supported GitHub plans.
- Enable immutable releases.
- Until Developer ID signing and notarization are implemented, state in the release notes that the macOS archives are unsigned.
