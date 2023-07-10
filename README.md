# Proteus

<img src="https://github.com/unblockable/proteus/blob/main/assets/proteus_white.png?raw=true" width="200" height="200">

Debug build (also used for tests):

    cargo build

Release build (optimized):

    cargo build --release

Run unit tests:

    cargo test

Run integration tests (need [`shadow`, `tgen`, `tor`, `python3`] in $PATH,
output stored in `target/tests/...`):

    cargo test -- --ignored

Run proteus while logging to stderr:

    RUST_LOG={error,warn,info,debug,trace} cargo run

# Docs

Pluggable Transports
- https://gitweb.torproject.org/torspec.git/tree/pt-spec.txt

Extended OR Port
- https://gitweb.torproject.org/torspec.git/tree/ext-orport-spec.txt

# Research

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
