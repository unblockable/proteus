# UPGen

Debug build (also used for tests):

    cargo build

Release build (optimized):

    cargo build --release

Run tests serially (because we modify environment variables):

    cargo test -- --test-threads=1

Run upgen while logging to stderr:

    RUST_LOG={error,warn,info,debug,trace} cargo run

# Docs

Pluggable Transports
- https://gitweb.torproject.org/torspec.git/tree/pt-spec.txt

Extended OR Port
- https://gitweb.torproject.org/torspec.git/tree/ext-orport-spec.txt
