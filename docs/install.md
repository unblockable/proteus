# Installation

This file documents the installation process of Proteus.  
Follow the following steps to build proteus from source:

## Prerequisites

- For debian based OSes

```
sudo apt install -y build-essential pkg-config libssl-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

- For arch based OSes

```
sudo pacman -S base-devel pkgconf openssl rustup
rustup toolchain install 1.87
```

---

## Procedure

1. Clone the repo and enter the repo directory

```
git clone https://github.com/unblockable/proteus.git
cd proteus
```

2. Build latest release of proteus 

```
cargo build --release
```

or without release build

```
cargo build
```

3. Verify the installation

```
./target/release/proteus --help
```

This completes the installation process of Proteus.

---
