# Tor Bridge Setup

This document guides user for deploying Proteus as a Tor bridge.
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

3. Add binding permissions to Proteus and Tor.  
(required for ports <1024)

```
[~/bridge/proteus] $ sudo setcap cap_net_bind_service=+eip target/release/proteus
[~/bridge/proteus] $ sudo setcap cap_net_bind_service=+eip /usr/bin/tor
```

4. Go to bridge directory and create a new directory for data, then change the directory permissions. We name it data for convinience, users can pick any name.

```
[~/bridge/proteus] $ cd ..
[~/bridge] $ mkdir -p data
[~/bridge] $ chmod 700 ~/bridge/data
```

5. Create a torrc file and fill the details

```
[~/bridge] $ nano torrc
```

The details of torrc are given in [torrc.md](torrc.md) file.

6. After writing the torrc, run it using 

```
/usr/bin/tor -f ~/bridge/torrc
```

This completes the setup of Proteus as tor bridge.

---
