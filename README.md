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

Run web server, pt server, and pt client in separate terminals:

    python3 -m http.server 8080
    ptadapter -S pta.conf -vv 2>&1 | grep -v unknown
    ptadapter -C pta.conf -vv 2>&1 | grep -v unknown

Check the CMETHOD output from the `ptadapter -C ...` command to find the
socks5 server port. Suppose the socks5 port is 66666, then run curl client:

    curl --socks5 127.0.0.1:66666 http://127.0.0.1:7999

What happens here:

- Curl will connect to the socks5 server running in the pt client at
  127.0.0.1:66666
- Curl will ask the pt client (via socks5) to connect to the pt server at
  127.0.0.1:7999
- The pt server will forward the data sent by curl directly to the http server
  at 127.0.0.1:8080 (via the `forward` directive in `pta.conf`)
- Curl should print out the contents of the dir where you ran the http server

# Docs

Pluggable Transports
- https://gitweb.torproject.org/torspec.git/tree/pt-spec.txt

Extended OR Port
- https://gitweb.torproject.org/torspec.git/tree/ext-orport-spec.txt

PTAdapter (for integration testing during dev)
- https://ptadapter.readthedocs.io/en/latest/console_script.html
