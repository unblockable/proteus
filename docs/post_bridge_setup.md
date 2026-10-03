# Post Bridge

This document helps bridge operators after the setup.

---

## Fingerprint

To view the fingerprint 

```
cat ~/bridge/data/fingerprint
```

---

## Running

After setup, operators can just run their bridge using one command only.

```
/usr/bin/tor -f ~/bridge/torrc
```

---

## Stopping

To stop the bridge, either press Ctrl+C or kill the process

```
pkill -f "tor -f ~/bridge/torrc"
```

---
