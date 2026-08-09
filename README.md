# whowns

[![CI](https://github.com/shm11C3/whowns/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/shm11C3/whowns/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/shm11C3/whowns?sort=semver)](https://github.com/shm11C3/whowns/releases/latest)
[![Platforms: macOS and Linux](https://img.shields.io/badge/platforms-macOS%20%7C%20Linux-blue)](#current-boundaries)
[![License: MIT](https://img.shields.io/github/license/shm11C3/whowns)](LICENSE)

[Japanese](README.ja.md)

`whowns` answers a deceptively simple question: "What installed and manages this command?"

The name compresses `who owns` into six characters. The tool focuses on explaining one command at a time instead of treating package inventory as its primary purpose. It answers:

- Which executable is active?
- Are other versions shadowed later in `PATH`?
- Which version manager or package manager owns the runtime?
- How was that manager itself installed?
- Which paths, symlinks, receipts, and manager queries support the conclusion?
- Which manager and command should be used to inspect, update, or remove it?

`whowns` is a standalone Rust binary. Users do not need Node.js, Python, or another runtime to run it.

## Table of contents

- [Installation](#installation)
- [Inspect a command](#inspect-a-command)
- [List common runtimes](#list-common-runtimes)
- [JSON](#json)
- [Confidence](#confidence)
- [Recognized owners](#recognized-owners)
- [Development](#development)
- [Current boundaries](#current-boundaries)

## Installation

[GitHub Releases](https://github.com/shm11C3/whowns/releases) is the primary binary distribution channel. The recommended installer detects the host OS and CPU, downloads the matching archive, verifies it against the release checksum manifest, and installs the standalone binary.

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/shm11C3/whowns/releases/latest/download/install.sh | sh
```

The default installation creates `$HOME/.local/bin/whowns` and the shorthand alias `$HOME/.local/bin/wio`. Pass installer options after `sh -s --`.

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/shm11C3/whowns/releases/latest/download/install.sh \
  | sh -s -- --no-alias

curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/shm11C3/whowns/releases/latest/download/install.sh \
  | sh -s -- --bin-dir /usr/local/bin
```

You can also download and inspect a release archive before installing it.

```sh
tar -xzf whowns-v0.1.0-aarch64-apple-darwin.tar.gz
cd whowns-v0.1.0-aarch64-apple-darwin
./install.sh
```

See [docs/RELEASING.md](docs/RELEASING.md) for supported architectures, checksums, artifact attestations, and the release process.

You can also install from source with Cargo.

```sh
cargo install --path . --locked

whowns node
```

## Inspect a command

```console
$ whowns node
node
├── ● active
│   ├── executable: /usr/local/bin/node
│   ├── ownership: node → macOS Installer (.pkg) [confirmed]
│   └── actions (macOS Installer (.pkg))
│       ├── inspect: pkgutil --pkg-info org.nodejs.node.pkg
│       └── note: Update by installing a newer package from the same vendor. ...
└── ○ shadowed
    ├── executable: /opt/homebrew/bin/node
    ├── resolves to: /opt/homebrew/Cellar/node/25.6.1_1/bin/node
    ├── ownership: node → Homebrew [probable]
    └── actions (Homebrew)
        ├── inspect: brew info node
        ├── update: brew upgrade node
        └── remove: brew uninstall node
```

The terminal tree keeps resolutions, ownership, and suggested actions visually connected. `active` is the executable selected by `PATH`. `shadowed` executables are installed but lose because they appear later in `PATH`. Action guides are suggestions only; `whowns` never runs update or removal commands.

`whowns` does execute other programs for inspection: recognized package and version managers (`mise which`, `pyenv which`, `pkgutil --file-info`, and similar) are invoked with read-only subcommands to confirm ownership. See [Recognized owners](#recognized-owners) for the safety policy around those queries.

Use `--explain` to show detailed evidence and every ownership layer.

```sh
whowns node --explain
whowns rustc cargo --explain
```

When the environment provides enough evidence, an ownership chain can look like this:

```text
node → nvm [probable] → Homebrew [probable]
rustc → rustup [confirmed] → rustup installer [probable]
```

Resolution continues upstream until an installation source is reached. Cycles and chains longer than eight owners stop with an `unconfirmed source` node whose evidence explains why traversal ended.

## List common runtimes

The summary is generated from the same `OwnershipGraph` used by individual inspection. The graph is the command-level model: it can contain multiple PATH resolutions, while each resolution contains one linear, nearest-first ownership chain. There is no separate ownership detector for this view.

```sh
whowns --all
whowns --all --explain
whowns --all --show-missing
```

The summary shows the active executable's owner chain, confidence, and shadowed count. `--explain` appends the detailed view of the same graphs.

## JSON

Individual inspection and `--all` emit the same machine-readable model.

```sh
whowns node --json
whowns --all --json
```

```text
OwnershipGraph (command and its PATH resolutions)
└── Resolution[] (active / shadowed, path, real_path)
    └── OwnershipNode[] (ordered nearest-first)
        ├── id (stable) / name (display)
        ├── kind
        ├── package / version
        ├── Confidence
        ├── Evidence[]
        └── ActionGuide
```

- `Resolution`: the active executable and every shadowed executable found in `PATH`
- `OwnershipNode`: one member of an ordered, nearest-first `runtime -> manager -> upstream manager -> installation source` chain
- `id`: the stable machine-readable owner identity, such as `homebrew`, `sdkman`, or `macos_installer`; `name` is display text and can change without affecting `id`
- `Evidence`: paths, symlinks, filesystem targets, receipts, package queries, and manager queries
- `Confidence`: `confirmed`, `probable`, or `unknown`
- `ActionGuide`: suggested inspect, update, and removal commands with safety notes

## Confidence

- `confirmed`: a package database or receipt records ownership of the file, or a manager query returns the resolved executable
- `probable`: a recognized managed-path layout, installed but non-file-specific receipt, or operating-system path strongly suggests an owner without a direct ownership record
- `unknown`: no recognized owner was found, so a safe update or removal method cannot be selected

Confidence is derived from the typed evidence on each ownership claim. Detectors report what they observed; they cannot assign `confirmed` directly. `--explain` shows the receipt, package query, matching manager query, or weaker path evidence behind the result.

A file under `/usr/local` is not automatically labeled as manually installed. Vendor installers, package managers, and manual copies can all use that location. Without stronger evidence, `whowns` reports `unconfirmed owner` and does not generate update or removal commands.

## Recognized owners

- Nix, Homebrew, and MacPorts
- nvm, fnm, Volta, mise, and asdf
- pyenv, rbenv, SDKMAN!, uv, rustup, and `cargo install`
- Deno and Bun installer directories, and pnpm home
- macOS Installer receipts through `pkgutil` and python.org framework installations
- Linux packages owned by dpkg, RPM, pacman, or apk
- operating-system paths

For supported managers, `whowns` runs read-only queries such as `which` or `current` and records their results as `Evidence`. For a path under the MacPorts prefix, `port -q provides <path>` checks the local registry; the path alone is `probable`, while a registry match is `confirmed`.

These queries run through a single bounded execution policy, not raw, unbounded subprocess calls:

- Each query targets the executable `whowns` already resolved on `PATH`, not a bare command name handed to a fresh `PATH` search. This keeps the query pointed at the same binary `whowns` inspected, even if `PATH` changes between the two lookups.
- Each query is killed if it does not finish within a few seconds, so a hung or slow manager cannot block a lookup or `--all`.
- Identical queries are only ever executed once per `whowns` invocation; repeated queries across runtimes or resolutions in `--all` reuse the cached result.
- Captured output is bounded; a manager that misbehaves and writes excessive output cannot exhaust memory.
- A timed-out or unstartable query is always printed as a `note:` line on stderr. A manager query — the confirmatory `which`/`current` query run after an owner is already identified — additionally records its outcome, including a non-zero exit, as `Evidence` on that owner, so a degraded confirmatory query stays visible instead of silently changing the result.
- Queries inherit the parent process environment unmodified. Managers resolve their own data directories from variables such as `HOME`; clearing or fabricating environment state for them would make their answers wrong rather than safer.

## Development

```sh
cargo test
cargo build --release
./target/release/whowns node
```

The project has no external Rust crate dependencies. Documentation and code comments use English; [README.ja.md](README.ja.md) is the Japanese localization.

## Current boundaries

`whowns` inspects executables found in `PATH` on macOS and Linux. It does not inventory every package registered with the operating system or package managers. Windows, tracing the source of shell configuration, and automatic uninstallation are currently out of scope.

If the installation source of a version manager cannot be determined, the ownership chain ends with `unconfirmed source [unknown]`.
