# lindesk

Low-latency remote desktop for Linux with a native **Wayland host**, written in Rust.
Share your screen and let someone control it, with a polished desktop app that updates itself.

- **Wayland-native capture** through xdg-desktop-portal ScreenCast and PipeWire.
- **Hardware H.264** (VA-API, NVENC) tuned for latency; x264 as fallback.
- **UDP transport** with ChaCha20-Poly1305 encryption, Reed-Solomon FEC, and frame dropping instead of queuing, so it recovers from loss without lag piling up.
- **Input control** through uinput, so it works on any compositor.
- **One-code pairing:** the host shows a connection code; the other side pastes it.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/LinCastr/lindesk/main/install.sh | bash
```

Installs to `~/.local/bin` (no root) and adds an app-menu entry. After that lindesk checks GitHub for a newer release every time it opens, verifies its signature and updates itself.
It uses your system libraries and does not bundle any. To remove it: `curl -fsSL https://raw.githubusercontent.com/LinCastr/lindesk/main/install.sh | bash -s -- --uninstall`.

Requirements: PipeWire, an xdg-desktop-portal backend with ScreenCast (`-hyprland`, `-kde`, `-gnome`, `-wlr`), GStreamer plugins (base, bad, PipeWire, an H.264 decoder). The app tells you what is missing.
To let the other person control your keyboard and mouse you need write access to `/dev/uinput` (`sudo usermod -aG input $USER`, then log in again).

## Use

1. On the computer to share: open lindesk → **Share this PC** → **Start sharing**, then **Copy code** and send it.
2. On the other computer: open lindesk → **Connect**, paste the code, **Connect**. F11 toggles full screen.

Same network or VPN works out of the box. Over the internet, forward the UDP port (default 7000) to the host on your router; NAT traversal is on the roadmap.

> The code contains the secret key. Anyone who has it can see and control the host. **Create new code** invalidates the old one.

## Command line

```bash
lindesk host --listen 0.0.0.0:7000        # headless sharing; key from LINDESK_KEY or the saved config
lindesk view LD-XXXXXX-...                # open a remote screen from a code
```

## Build

```bash
sudo apt install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev   # or your distro's equivalent
cargo build --release -p lindesk-app      # target/release/lindesk
```

## Releases

Push a tag `vX.Y.Z` that matches the workspace version in `Cargo.toml`. The workflow builds, signs the binary with the project's minisign key and publishes it. The public key is compiled into the app; the private key exists only as a repository secret.

## License

MIT
