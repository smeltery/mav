# mav

<img width="2048" height="1287" alt="10266" src="https://github.com/user-attachments/assets/7ab2fd83-2b38-45b9-a6e9-40d74d89dda2" />

[![CI](https://github.com/smeltery/mav/actions/workflows/ci.yml/badge.svg)](https://github.com/smeltery/mav/actions/workflows/ci.yml)
[![License: PolyForm Shield 1.0.0](https://img.shields.io/badge/license-PolyForm%20Shield%201.0.0-blue.svg)](LICENSE)
[![Platform: macOS + Linux](https://img.shields.io/badge/platform-macOS%20%2B%20Linux-lightgrey.svg)](docs/getting-started.md)
[![Rust: stable](https://img.shields.io/badge/rust-stable-orange.svg)](CONTRIBUTING.md)
[![pre-commit](https://img.shields.io/badge/pre--commit-enabled-brightgreen?logo=pre-commit&logoColor=white)](.pre-commit-config.yaml)
[![Dev env: Flox](https://img.shields.io/badge/dev--env-Flox-blueviolet.svg)](https://flox.dev)
[![Nix: flake](https://img.shields.io/badge/Nix-flake-blue.svg)](flake.nix)

`mav` keeps the desktop runtime, extension host, bundled languages, themes, 
settings, packaging scripts, and reproducible developer environments in one 
repository.

```console
$ gh repo clone smeltery/mav
$ cd mav
$ flox activate
$ cargo run -p mav --bin mav

# Nix users can enter the same pinned workspace:
$ nix develop
$ cargo check -p mav --bin mav --locked
```

See [docs/README.md](docs/README.md) for the full documentation index,
[docs/architecture.md](docs/architecture.md) for the workspace layout, and
[docs/getting-started.md](docs/getting-started.md) for first-run setup.

## Install

Clone the public repository:

```sh
gh repo clone smeltery/mav
cd mav
```

Use [Flox](https://flox.dev) for the fastest setup. It provides the pinned Rust
toolchain, formatters, link checker, pre-commit, GitHub CLI, and supporting
build tools:

```sh
flox activate
cargo check -p mav --bin mav --locked
```

Nix users can use the flake directly:

```sh
nix develop
cargo check -p mav --bin mav --locked
```

## Commands

| Command | What it does |
|---|---|
| `cargo run -p mav --bin mav` | Build and launch the desktop app from the workspace |
| `cargo check -p mav --bin mav --locked` | Type-check the app binary with the locked dependency graph |
| `cargo check -p feedback --locked` | Type-check the feedback/support crate used by CI |
| `cargo check -p mav_extension_api --locked` | Type-check the extension API crate |
| `cargo fmt --all -- --check` | Verify Rust formatting |
| `pre-commit run --all-files` | Run local repository hygiene checks |
| `lychee --offline README.md docs/**/*.md` | Check documentation links without touching the network |
| `flox activate` | Enter the pinned Flox development shell |
| `nix develop` | Enter the pinned Nix development shell |

## Development

```sh
flox activate
pre-commit run --all-files
cargo check -p mav --bin mav --locked
```

With [Flox](https://flox.dev) installed, `flox activate` drops you into a shell
with the pinned Rust toolchain, docs tooling, GitHub CLI, and repository hooks
already on `PATH`. With Nix installed, `nix develop` provides the same project
tooling through the checked-in flake.

See **[CONTRIBUTING.md](CONTRIBUTING.md)** for the contributor workflow,
**[docs/testing.md](docs/testing.md)** for the test suite,
**[docs/ci.md](docs/ci.md)** for what CI checks,
**[docs/extensions.md](docs/extensions.md)** for extension work,
**[docs/nix-and-flox.md](docs/nix-and-flox.md)** for reproducible development
environments, and **[docs/releasing.md](docs/releasing.md)** for release
handling.

[PolyForm Shield License 1.0.0](LICENSE).
