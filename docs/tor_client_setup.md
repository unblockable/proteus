# Client Setup

This document guides user for deploying Proteus as a Tor client.
We also provide the current directory info along with command to help better.

---

## Procedure

First, we assume that user is at `/home/user/` directory.

1. Create a directory (optional, recommended) and enter it.

```
[~/] $ mkdir -p bridge
[~/] $ cd bridge
```

2. Install the Proteus inside the bridge directory.  
Use [`Installation Guide`](install.md) for help.

3. Find the torrc file of Tor browser

```
[~/bridge] find ~ -iname "torrc" 2>/dev/null
```

Note that the torrc file of Tor browser is different from `/etc/tor/torrc`

4. Add Proteus usage to Torrc

```
ClientTransportPlugin proteus exec ~/bridge/proteus/target/release/proteus pt
```

5. Get a PSF and put it in bridge directory. We assume the psf is at bridge/ directory as "file.psf"

6. Go to Tor browser and enter details manually for bridge

```
proteus <ip_address>:port <psf_fingerprint> psf=/home/user/bridge/file.psf
```

Connect to Tor and visit any site.

This completes the client setup for tor.

---
