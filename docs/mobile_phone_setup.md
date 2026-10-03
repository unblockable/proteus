# Termux

This document helps users in running Proteus in their mobila phones via termux.
We assume the user is at home (~)

---

## Procedure 

1. First install the prerequisites

```
[~/] $ pkg install tor torsocks curl
```

2. Create a directory and enter (optional, recommended)

```
[~/] $ mkdir -p bridge
[~/] $ cd bridge
```

3. Install Proteus. Please follow the following steps instead of installation guide.

```
[~/bridge] $ pkg install git clang lld rust make pkg-config openssl
[~/bridge] $ git clone https://github.com/unblockable/proteus.git
[~/bridge] $ cd proteus
[~/bridge/proteus] $ cargo build --release
```

4. Go back to bridge directory and create torrc file

```
[~/bridge/proteus] $ cd ..
[~/bridge] $ nano torrc
```

5. Edit torrc and add the info:

```
ClientTransportPlugin proteus exec /data/data/com.termux/files/home/bridge/proteus/target/release/proteus pt
Bridge proteus xx.xx.xx.xx:yyyy psf=/data/data/com.termux/files/home/path/to/file.psf
SocksPort 127.0.0.1:9150
DataDirectory /data/data/com.termux/files/home/bridge/data
```

Note that using "~" instead of `/data/data/com.termux/files/home/` will return errors while running torrc. 
So, write full address instead of using ~ only.

6. Create the data directory and edit permissions

```
[~/bridge] $ mkdir -p data
[~/bridge] $ chmod 700 data
```

7. Run torrc

```
tor -f torrc
```

8. Keep it running and test the connection in a new tab

```
export TORSOCKS_TOR_PORT=9150
torsocks curl https://www.torproject.org/
```

or

```
curl --socks5-hostname 127.0.0.1:9150 https://www.torproject.org/
```

This completes the procedure.

---
