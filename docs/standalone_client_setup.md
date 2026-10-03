# Standalone Client Setup

This document guides user for deploying Proteus as a standalone client proxy.
We used port 8099 for this guide, but users can pick any unused port.

---

## Procedure

1. Install Proteus using the [Installation Guide](install.md).

2. Start Proteus client and choose a local listening address.

```
[~/bridge/proteus] $ ./target/release/proteus client /path/to/file.psf --connect xx.xx.xx.xx:yyyy --listen 127.0.0.1:8099
```

2. Verify the use of proxy.

```
[~/] $ curl --socks5-hostname 127.0.0.1:8099 https://www.torproject.org/
```

(or any other website of choice)

3. Applications that support proxy settings can be configured with:

```
SOCKS5 proxy: 127.0.0.1
SOCKS5 port: 8099
```

This completes the standalone Proteus client setup.

---
