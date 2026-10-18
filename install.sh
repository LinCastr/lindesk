#!/usr/bin/env bash
# lindesk installer (Linux x86_64):
#   curl -fsSL https://raw.githubusercontent.com/LinCastr/lindesk/main/install.sh | bash
# Options (after `bash -s --`):  --prefix DIR   --version vX.Y.Z   --no-desktop   --uninstall
#
# Installs one binary into ~/.local/bin (no root needed), plus an app-menu entry.
# The app keeps itself up to date from GitHub releases, verifying a signature each time.
# It uses the system's own libraries: nothing is bundled and no loader or
# LD_LIBRARY_PATH tricks are involved.
set -euo pipefail

REPO="LinCastr/lindesk"
ASSET="lindesk-linux-x86_64"
PREFIX="${HOME}/.local"
VERSION="latest"
DESKTOP=1
UNINSTALL=0

die() { echo "error: $*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix) PREFIX="${2:?--prefix needs a directory}"; shift 2 ;;
    --version) VERSION="${2:?--version needs a tag like v0.1.0}"; shift 2 ;;
    --no-desktop) DESKTOP=0; shift ;;
    --uninstall) UNINSTALL=1; shift ;;
    -h|--help) sed -n '2,8p' "$0" 2>/dev/null || true; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

BIN_DIR="$PREFIX/bin"
APP_DIR="$PREFIX/share/applications"
ICON_DIR="$PREFIX/share/icons/hicolor/256x256/apps"

if [[ $UNINSTALL -eq 1 ]]; then
  rm -f "$BIN_DIR/lindesk" "$BIN_DIR/lindesk.old" "$APP_DIR/lindesk.desktop" "$ICON_DIR/lindesk.png"
  echo "lindesk removed. Settings stay in ~/.config/lindesk (delete that folder to remove them too)."
  exit 0
fi

[[ "$(uname -s)" == "Linux" ]] || die "Linux only"
case "$(uname -m)" in x86_64|amd64) ;; *) die "only x86_64 builds are published" ;; esac
for cmd in curl sha256sum install; do command -v "$cmd" >/dev/null || die "missing '$cmd'"; done

if [[ "$VERSION" == "latest" ]]; then
  BASE="https://github.com/$REPO/releases/latest/download"
else
  BASE="https://github.com/$REPO/releases/download/$VERSION"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading lindesk ($VERSION)..."
curl -fL --progress-bar -o "$tmp/$ASSET" "$BASE/$ASSET" || die "download failed (is there a release yet?)"
curl -fsSL -o "$tmp/SHA256SUMS" "$BASE/SHA256SUMS" || die "could not download the checksum list"

expected="$(awk -v f="$ASSET" '$2==f {print $1}' "$tmp/SHA256SUMS")"
actual="$(sha256sum "$tmp/$ASSET" | awk '{print $1}')"
[[ -n "$expected" && "$expected" == "$actual" ]] || die "checksum mismatch, aborting"

# Extra check when minisign is installed (the app itself always verifies updates).
if command -v minisign >/dev/null; then
  curl -fsSL -o "$tmp/$ASSET.minisig" "$BASE/$ASSET.minisig" \
    && minisign -Vm "$tmp/$ASSET" -P "RWRGwnBOFH4dnWuk6H0kLQmOaUC7sTY/dKJE8k7OSyrhVQegRZGazwrt" -x "$tmp/$ASSET.minisig" >/dev/null \
    || die "signature check failed, aborting"
fi

mkdir -p "$BIN_DIR"
install -m 755 "$tmp/$ASSET" "$BIN_DIR/lindesk.new"
mv -f "$BIN_DIR/lindesk.new" "$BIN_DIR/lindesk"

if [[ $DESKTOP -eq 1 ]]; then
  mkdir -p "$APP_DIR" "$ICON_DIR"
  curl -fsSL -o "$ICON_DIR/lindesk.png" "$BASE/lindesk.png" || true
  cat > "$APP_DIR/lindesk.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=lindesk
Comment=Low-latency remote desktop
Exec=$BIN_DIR/lindesk
Icon=lindesk
Terminal=false
Categories=Network;RemoteAccess;
StartupWMClass=lindesk
DESK
  command -v update-desktop-database >/dev/null && update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
fi

echo
if ! out="$("$BIN_DIR/lindesk" --version 2>&1)"; then
  echo "Installed, but the program did not start:"
  echo "  $out"
  echo "A shared library is probably missing; see the packages below."
else
  echo "Installed $out to $BIN_DIR/lindesk"
fi

distro=""
[[ -r /etc/os-release ]] && distro="$(. /etc/os-release; echo "${ID:-} ${ID_LIKE:-}")"
echo
echo "lindesk needs PipeWire, an xdg-desktop-portal with ScreenCast support (xdg-desktop-portal-hyprland / -kde / -gnome / -wlr) and GStreamer plugins. If something is missing:"
case "$distro" in
  *arch*) echo "  sudo pacman -S --needed pipewire gst-plugin-pipewire gst-plugins-base gst-plugins-bad gst-plugin-va gst-libav" ;;
  *debian*|*ubuntu*) echo "  sudo apt install pipewire gstreamer1.0-pipewire gstreamer1.0-plugins-base gstreamer1.0-plugins-bad gstreamer1.0-libav gstreamer1.0-vaapi" ;;
  *fedora*) echo "  sudo dnf install pipewire gstreamer1-plugins-base gstreamer1-plugins-bad-free pipewire-gstreamer gstreamer1-vaapi" ;;
  *) echo "  install: pipewire, the GStreamer PipeWire plugin, gst-plugins-base, gst-plugins-bad (VA-API/NVENC) and an H.264 decoder" ;;
esac
echo "Control of the keyboard and mouse needs write access to /dev/uinput:  sudo usermod -aG input \$USER  (then log out and in)"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo; echo "Note: $BIN_DIR is not in your PATH. Start lindesk from the app menu, or add it to PATH." ;;
esac
echo
echo "Start it from your app menu, or run: lindesk"
