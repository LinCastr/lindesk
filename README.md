# lindesk

Low-latency remote desktop for Linux with a native **Wayland host**, written in Rust.
Share your screen and let someone control it, with a polished desktop app that updates itself.

- **Wayland-native capture** through xdg-desktop-portal ScreenCast and PipeWire.
- **Hardware H.264** (VA-API, NVENC) tuned for latency; x264 as fallback.
- **UDP transport** with Noise (X25519, ChaCha20-Poly1305) encryption, Reed-Solomon FEC, and frame dropping instead of queuing, so it recovers from loss without lag piling up.
- **Input control** through uinput, so it works on any compositor.
- **AnyDesk-style pairing:** a 10-digit ID, no accounts, no server; the host owner accepts each new computer.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/LinCastr/lindesk/main/install.sh | bash
```

Installs to `~/.local/bin` (no root) and adds an app-menu entry. After that lindesk checks GitHub for a newer release every time it opens, verifies its signature and updates itself.
It uses your system libraries and does not bundle any. To remove it: `curl -fsSL https://raw.githubusercontent.com/LinCastr/lindesk/main/install.sh | bash -s -- --uninstall`.

Requirements: PipeWire, an xdg-desktop-portal backend with ScreenCast (`-hyprland`, `-kde`, `-gnome`, `-wlr`), GStreamer plugins (base, bad, PipeWire, an H.264 decoder). The app tells you what is missing.
To let the other person control your keyboard and mouse you need write access to `/dev/uinput` (`sudo usermod -aG input $USER`, then log in again).

## Use

1. On the computer to share: open lindesk → **Share this PC** → **Start sharing**. It shows a 10-digit **ID**; tell it to the other person.
2. On the other computer: open lindesk → **Connect**, type the ID, **Connect**.
3. The first computer sees "X wants to connect": **Accept** (once), **Always allow** (that computer will not be asked again) or **Reject**. F11 toggles full screen in the viewer.

The ID is your IPv4 address as one number, picked automatically: your VPN if you have one (Radmin, Hamachi, Tailscale, Netbird, WireGuard…), otherwise your local network. It works for whoever is on that same VPN or network. Over the open internet, forward the UDP port (default 7000) to the host on your router; NAT traversal is on the roadmap.

**Security:** there is no password. Nobody gets in without the host owner's click, and everything is encrypted (Noise XX, ChaCha20-Poly1305) with each computer's own key. Each side shows a 4-digit verification code; the first time you reach an ID the app remembers that computer's key and refuses a different one later. Trusted computers can be removed in Settings. The app is in Portuguese or English (Settings).

## Command line

```bash
lindesk host --listen 0.0.0.0:7000        # headless sharing; answer y / a / n to connection requests
lindesk view 0441330607                   # open a remote screen from an ID (add :port if it is not 7000)
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
