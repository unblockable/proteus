# Tor SOCKS Client Setup

This document guides user for testing a Proteus bridge through Tor's local
SOCKS5 proxy.
It may be helpful for those who don't want to change the Tor bundle and want to test Proteus directly via CLI.

## Procedure

We assume the user is at `/home/user` directory.

1. Create a directory and enter it (optional, recommended)

```
[~/] $ mkdir -p bridge
[~/] $ cd bridge
```

2. Install Proteus using the [`Installation Guide`](install.md) inside bridge.

3. Create a torrc. Note that we don't use systemwide torrc, because it doesn't work by default due to permission problems. 

```
[~/bridge] $ touch torrc
[~/bridge] $ nano torrc
```

4. In the torrc file, add Proteus as Tor pluggable transport and also add the bridge line.
The port 9050 is already used by Tor, so we use port 9150. Add it to torrc.

```
ClientTransportPlugin proteus exec /home/user/bridge/proteus/target/release/proteus pt
Bridge proteus xx.xx.xx.xx:yyyy psf=/path/to/file.psf
SocksPort 127.0.0.1:9150
```

5. Start the process and keep it running.

```
[~/] $ tor -f ~/bridge/torrc
```

6. In a new tab, test the connection with torsocks. For that, we first need to set port, then test site.

```
[~/bridge] $ export TORSOCKS_TOR_PORT=9150
[~/bridge] $ torsocks curl https://www.torproject.org/
```

This completes the Tor SOCKS client setup.

---
