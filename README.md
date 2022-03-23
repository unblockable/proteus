# UPGen

Debug build (also used for tests):

    cargo build

Release build (optimized):

    cargo build --release

Run tests serially (because we modify environment variables):

    cargo test -- --test-threads=1

Run upgen while logging to stderr:

    RUST_LOG={error,warn,info,debug,trace} cargo run

# Integration Testing

Use ptadapter to wrap upgen over localhost.

Install ptadapter

    python3 -m venv ptenv
    source ptenv/bin/activate
    pip install ptadapter

Set up ptadapter config, `pta.conf`:

    [client]

    exec = /Users/rjansen/Documents/scratch/upgen/target/debug/upgen
    state = 
    tunnels = client_upgen_1

    [client_upgen_1]

    transport = upgen
    listen = 127.0.0.1:8000
    upstream = 127.0.0.1:7999
    options-seed = 12345

    [server]

    exec = /Users/rjansen/Documents/scratch/upgen/target/debug/upgen
    state = 
    forward = 127.0.0.1:8080
    tunnels = server_upgen_1

    [server_upgen_1]

    transport = upgen
    listen = 127.0.0.1:7999
    options-seed = 54321

Run web server, pt server, pt client, and web client in separate terminals:

    python3 -m http.server 8080
    ptadapter -S pta.conf -vv
    ptadapter -C pta.conf -vv
    # not sure about this curl command
    curl --socks5 127.0.0.1:8000 http://127.0.0.1:8080

# Docs

Pluggable Transports
- https://gitweb.torproject.org/torspec.git/tree/pt-spec.txt

Extended OR Port
- https://gitweb.torproject.org/torspec.git/tree/ext-orport-spec.txt

PTAdapter (for integration testing during dev)
- https://ptadapter.readthedocs.io/en/latest/console_script.html
