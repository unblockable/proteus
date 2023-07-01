# Proteus

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
