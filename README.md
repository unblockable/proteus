# Proteus

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="/assets/proteus_white.png?raw=true">
  <source media="(prefers-color-scheme: light)" srcset="/assets/proteus_black.png?raw=true">
  <img alt="Proteus symbol" src="/assets/proteus_bg.png?raw=true" align="right" width="200" height="200">
</picture>

[![Lint Checks](https://github.com/unblockable/proteus/actions/workflows/lint.yml/badge.svg)](https://github.com/unblockable/proteus/actions/workflows/lint.yml)
[![Unit and Integration Tests](https://github.com/unblockable/proteus/actions/workflows/unit_integration.yml/badge.svg)](https://github.com/unblockable/proteus/actions/workflows/unit_integration.yml)
[![System Tests in Shadow](https://github.com/unblockable/proteus/actions/workflows/system.yml/badge.svg)](https://github.com/unblockable/proteus/actions/workflows/system.yml)

Proteus is a tool for establishing network communication tunnels to a proxy
server using network protocols that can be programmed by users. 

> [!CAUTION]
> Proteus is experimental and not recommended for security-critical applications.

## Easy install

    cargo install --git https://github.com/unblockable/proteus
    proteus --help

## Usage

Proteus can currently be run as a pluggable transport using the v1
specification. The main configuration item needed is a protocol specification
file (PSF) which proteus will compile and run for each connection between a
client and proxy server. See `tests/fixtures` for example PSFs.

## Development notes

Debug build (also used for tests):

    cargo build

Release build (optimized):

    cargo build --release

Run tests:

    cargo test

Run system tests (need [`shadow`, `tgen`, `tor`, `python3`] in $PATH,
output stored in `target/tests/...`):

    cargo test -- --ignored

Maintain standard code formatting:

    cargo +nightly fmt -- --config-path rustfmt-nightly.toml

Maintain clippy standards:

    cargo clippy --all-targets -- -Dwarnings

Pluggable Transport v1 specifications
- https://gitweb.torproject.org/torspec.git/tree/pt-spec.txt
- https://gitweb.torproject.org/torspec.git/tree/ext-orport-spec.txt

## Research notes

You can read more technical details about our vision for Proteus in the
following publication:

[Proteus: Programmable Protocols for Censorship Circumvention](https://www.petsymposium.org/foci/2023/foci-2023-0013.pdf).  
[Ryan Wails](https://ryanwails.com/),
[Rob Jansen](https://www.robgjansen.com),
[Aaron Johnson](https://www.ohmygodel.com/), and
[Micah Sherr](https://seclab.cs.georgetown.edu/msherr/).  
[Workshop on Free and Open Communication on the Internet, 2023](https://foci.community/foci23.html).

Cite our work:

```
@inproceedings{proteus-foci2023,
  title = {Proteus: Programmable Protocols for Censorship Circumvention},
  author = {Wails, Ryan and Jansen, Rob and Johnson, Aaron and Sherr, Micah},
  booktitle = {Workshop on Free and Open Communication on the Internet},
  year = {2023},
}
```
