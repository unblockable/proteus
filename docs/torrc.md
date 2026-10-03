# Torrc

This file helps users in filling the torrc file for Proteus

```
ContactInfo User                                                 # Operator contact info
Nickname Proteus                                                 # Human-readable nickname 

Address xx.xx.xx.xx                                              # IPv4 address
OutboundBindAddress xx.xx.xx.xx                                  # Source address used for Tor's outgoing connections
ORPort xx.xx.xx.xx:yyyy                                          # Address and port for Tor relay connections
AssumeReachable 1                                                # Skip self-reachability checks; advertise immediately

ExitRelay 0                                                      # Keep it 0 always for proteus bridge
ExitPolicy reject *:*                                            # Reject exit traffic
SocksPort 0                                                      # Tor's local SOCKS proxy port

BridgeRelay 1                                                    # Operate as a bridge
PublishServerDescriptor 0                                        # Do not publish bridge descriptor publicly
ExtORPort 0                                                      # Extended ORPort
ServerTransportPlugin proteus exec /path/to/proteus pt           # Launch Proteus as the server transport
ServerTransportListenAddr proteus xx.xx.xx.xx:443                # Have Proteus listen for bridge traffic on TCP 443
ServerTransportOptions proteus psf=/path/to/file.psf             # Write the PSF along with the address

MaxMemInQueues x GB                                              # Cap memory used by Tor's message queues
NumCPUs x                                                        # Number of CPU workers Tor should use
LogTimeGranularity x msec                                        # Round log timestamps to x millisecond
ShutdownWaitLength x seconds                                     # Wait up to x second during graceful shutdown

DataDirectory /home/user/bridge/data                             # Directory for Tor state, keys, and cached data
PidFile /home/user/bridge/data/tor.pid                           # File containing Tor's process ID
Log notice file /home/user/bridge/data/notice.log                # Write notice-and-higher priority logs to this file
```

---

## Example

Users can fill the details using reference.  
Please do NOT fill exact same details as following, only use as reference.  

```
ContactInfo Ajay
Nickname Proteus

Address 66.116.244.138
OutboundBindAddress 66.116.244.138
ORPort 66.116.244.138:9001
AssumeReachable 1

ExitRelay 0
ExitPolicy reject *:*
SocksPort 0

BridgeRelay 1
PublishServerDescriptor 0
ExtORPort 0
ServerTransportPlugin proteus exec /home/ajay/bridge/proteus/target/release/proteus pt
ServerTransportListenAddr proteus 66.116.244.138:443
ServerTransportOptions proteus psf=/home/ajay/bridge/proteus/tests/fixtures/tls_mimic.psf

MaxMemInQueues 1 GB
NumCPUs 1
LogTimeGranularity 1 msec
ShutdownWaitLength 1 seconds

DataDirectory /home/ajay/bridge/data
PidFile /home/ajay/bridge/data/tor.pid
Log notice file /home/ajay/bridge/data/notice.log
```

---
