# lindesk

Low-latency remote desktop for Linux with a native **Wayland host**, written in Rust.

- Capture: xdg-desktop-portal ScreenCast + PipeWire (the screen choice is remembered).
- Encode: hardware H.264 (VA-API, NVENC) with low-latency settings; x264 fallback.
- Transport: UDP, ChaCha20-Poly1305 with a pre-shared key, Reed-Solomon FEC, frame dropping instead of queuing, loss-driven bitrate control.
- Input: uinput on the host, so it works on any compositor.

## Usage

```bash
cargo build --release
target/release/lindesk-host --listen 0.0.0.0:7000   # prints the key file path on start
target/release/lindesk-client HOST_IP:7000 --key-file path/to/key
```

The key is created on the first host run at `~/.config/lindesk/key`. Copy it to the client over a trusted channel.
Status: early prototype; NAT traversal is not implemented yet (use a LAN, VPN or an open UDP port).
