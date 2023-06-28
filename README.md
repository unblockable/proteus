# Proteus

Debug build (also used for tests):

    cargo build

Release build (optimized):

    cargo build --release

Run tests serially (because we modify environment variables):

    cargo test -- --test-threads=1

Run proteus while logging to stderr:

    RUST_LOG={error,warn,info,debug,trace} cargo run

# Shadow Integration Testing

If you have `shadow`, `tgen`, `tor`, and `python3` installed and in your PATH,
then cargo will build some shadow integration tests. Shadow tests are ignored by
default, but can be run with:

    cargo test -- --ignored

To run the shadow tests along with the unit tests:

    cargo test -- --test-threads=1 --include-ignored

You can inspect integration test output in `target/tests/...`

# Docs

Pluggable Transports
- https://gitweb.torproject.org/torspec.git/tree/pt-spec.txt

Extended OR Port
- https://gitweb.torproject.org/torspec.git/tree/ext-orport-spec.txt
