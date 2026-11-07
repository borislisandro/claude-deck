#!/usr/bin/env bash
# Install claude-deck for the current user (no root needed).
#
#   packaging/install.sh                 build, install the binary, widget, icon, launcher, shortcuts
#   packaging/install.sh --add-to-panel  also put the island on your top panel, left of the
#                                        first system monitor widget, as its own island
#   packaging/install.sh --uninstall     remove everything again
set -euo pipefail

cd "$(dirname "$0")/.."
BIN_DIR="$HOME/.local/bin"
BIN="$BIN_DIR/claude-deck"
APPS="$HOME/.local/share/applications"
WIDGET_ID="com.borislisandro.claudedeck"

say() { printf '\033[1;38;2;217;119;87m✻\033[0m %s\n' "$*"; }

if [[ "${1:-}" == "--uninstall" ]]; then
    "$BIN" quit 2>/dev/null || true
    kpackagetool6 -t Plasma/Applet -r "$WIDGET_ID" 2>/dev/null || true
    rm -f "$BIN" "$APPS/claude-deck.desktop" "$HOME/.local/share/kglobalaccel/claude-deck.desktop" \
          "$HOME/.local/share/icons/hicolor/scalable/apps/claude-deck.svg"
    rm -rf "$HOME/.local/share/claude-deck"
    say "Uninstalled. Your saved workspace is kept in ~/.local/state/claude-deck."
    exit 0
fi

for tool in kitty cargo kpackagetool6 gdbus; do
    command -v "$tool" >/dev/null || { echo "missing: $tool" >&2; exit 1; }
done

say "Building (release)…"
nice -n 19 ionice -c3 cargo build --release --quiet

say "Installing the binary to $BIN"
install -Dm755 target/release/claude-deck "$BIN"

say "Installing the panel widget"
if kpackagetool6 -t Plasma/Applet -s "$WIDGET_ID" >/dev/null 2>&1; then
    kpackagetool6 -t Plasma/Applet -u plasmoid/package >/dev/null
else
    kpackagetool6 -t Plasma/Applet -i plasmoid/package >/dev/null
fi

say "Installing the launcher and global shortcuts (Meta+C, Meta+Shift+C)"
install -Dm644 packaging/claude-deck.svg "$HOME/.local/share/icons/hicolor/scalable/apps/claude-deck.svg"
mkdir -p "$APPS" "$HOME/.local/share/kglobalaccel"
sed "s|@BIN@|$BIN|g" packaging/claude-deck.desktop > "$APPS/claude-deck.desktop"
ln -sf "$APPS/claude-deck.desktop" "$HOME/.local/share/kglobalaccel/claude-deck.desktop"
kbuildsycoca6 >/dev/null 2>&1 || true
# kglobalaccel reads launcher shortcuts at login; register them now too, the way
# System Settings does, so they work without logging out.
register_shortcut() { # action-id friendly-name qt-key-code
    local id="['claude-deck.desktop', '$1', 'Claude Deck', '$2']"
    gdbus call --session --dest org.kde.kglobalaccel --object-path /kglobalaccel \
        --method org.kde.KGlobalAccel.doRegister "$id" >/dev/null 2>&1 || return 0
    gdbus call --session --dest org.kde.kglobalaccel --object-path /kglobalaccel \
        --method org.kde.KGlobalAccel.setShortcutKeys "$id" "[([$3],)]" 4 >/dev/null 2>&1 || true
}
META=$((0x10000000)); SHIFT=$((0x02000000)); KEY_C=$((0x43))
register_shortcut _launch "Claude Deck" $((META + KEY_C))
register_shortcut NextWaiting "Jump to the Session Waiting Longest" $((META + SHIFT + KEY_C))

if [[ "${1:-}" == "--add-to-panel" ]]; then
    say "Adding the island to the top panel"
    script=$(cat <<'JS'
const id = "com.borislisandro.claudedeck";
let done = false;
for (const p of panels()) {
    if (done || p.location !== "top") continue;
    if (p.widgets(id).length > 0) { print("already on the panel"); done = true; break; }
    // Insert just left of the first system monitor (the CPU/GPU temperatures).
    const anchor = p.widgets("org.kde.plasma.systemmonitor")[0];
    let w;
    if (anchor) {
        const g = anchor.geometry;
        w = p.addWidget(id, g.x + 1, g.y + g.height / 2, 1, 1);
    } else {
        w = p.addWidget(id);
    }
    w.globalShortcut = "Meta+Alt+C";
    if (anchor) {
        // Panel Colorizer starts a new island at each spacer: keep the Claude island
        // separate from the system monitors with a small fixed spacer.
        const g = anchor.geometry;
        const s = p.addWidget("org.kde.plasma.panelspacer", g.x + 1, g.y + g.height / 2, 1, 1);
        s.currentConfigGroup = ["General"];
        s.writeConfig("expanding", false);
        s.writeConfig("length", 8);
    }
    if (p.widgets("luisbocanegra.panel.colorizer").length > 0) {
        // Colorizer draws the island; don't draw a second pill inside it.
        w.currentConfigGroup = ["General"];
        w.writeConfig("ownBackground", false);
    }
    done = true;
    print("added");
}
if (!done) print("no top panel found");
JS
)
    gdbus call --session --dest org.kde.plasmashell --object-path /PlasmaShell \
        --method org.kde.PlasmaShell.evaluateScript "$script" >/dev/null
fi

say "Done. Press Meta+C (or run: claude-deck) to open the deck."
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) say "Note: $BIN_DIR is not on your PATH." ;;
esac
