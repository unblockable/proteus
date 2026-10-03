# Standalone Proxy Setup

This document guides user for deploying Proteus as a standalone proxy server.
This setup is useful when testing Proteus without Tor Browser.

---

## Procedure

1. First, install Proteus using the [Installation Guide](install.md) file.

2. Start Proteus server and use a PSF.

```
[~/bridge/proteus] $ ./target/release/proteus server /home/user/bridge/file.psf --listen 0.0.0.0:yyyy
```

Operators can also change logging level if they want logs:

```
[~/bridge/proteus] $ ./target/release/proteus --log-level debug server /home/user/bridge/file.psf --listen 0.0.0.0:yyyy
```

This completes the standalone Proteus proxy server setup.

---
