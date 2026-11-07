# ✻ claude-deck

**A deck of Claude Code sessions for KDE Plasma, plus a live status island on your panel.**

When Claude finishes or needs an answer while you're in a fullscreen game, Plasma holds the
notification and nothing reminds you afterwards, so the session just sits there. claude-deck
keeps every session's state on the top bar, where you'll see it:

```
 ⌘ Chrome            Sat 7 Nov 13:33      ( 🦀● 🦀● 🦀● )  ( CPU 52°C  GPU 44°C )  ☁ 21°
                                             │    │    └ red     resting: finished, your turn (z's float up)
                                             │    └ yellow       thinking: needs an answer (thought bubble)
                                             └ green             running: Claude is working
```

Each session gets a little pixel critter on its own island. Its antenna glows in the session's
status color and its light sits right beside it.

The island only exists while the deck runs. Hover a session to peek at its pane, click it to jump there.

## What you get

- **One kitty window, many sessions.** The deck is kitty with its own config layered on top of
  yours (same font, theme and keys). Sessions live in tabs and splits, and each tab title shows
  `● 2 JobScraper 4m` in its session's status color.
- **Status from Claude Code hooks, not screen scraping.** Every deck session runs
  `claude --settings <deck hooks>`, so your own `~/.claude/settings.json` is never touched.
  Pressing Esc mid-turn is detected from the transcript, since Claude fires no hook for it.
- **The island.** One critter and status light per session, in the same order as the deck's tabs
  and panes (move a tab and the island follows):
  - the critter runs, with a breathing green light, while Claude works
  - it thinks, with a blinking yellow light and a ripple, when Claude needs you
  - it rests, with a red light that ripples until you've looked, when the turn is over
  - it turns grey when Claude has exited

  Waits over a minute show a timer. Six or more sessions collapse to per-state counts.
- **Hover:** a card with project, branch, state, last action, last prompt and the pane's last lines.
  The deck window is also highlighted, the same effect the task manager uses on hover.
- **Click:** focuses that session's pane and raises the deck. Clicking the session you're already
  in hides the deck. **Middle click:** new session. **Scroll:** switch session.
  **Click the pill:** popup with a live thumbnail of the deck and a card per session
  (Open · Interrupt · Mark seen · Close).
- **Never miss one:**
  - A notification fires when a session turns yellow or red while you're not looking; it has an
    Open action.
  - A reminder fires after 5 minutes unseen.
  - When a fullscreen app closes and something is waiting, you get one "While you were away"
    notification and the popup opens.
  - Optional: a KDE Connect push to your phone, and sounds.
- **Sessions survive mistakes.** When Claude exits or crashes, the pane stays open:
  `[r]` resume · `[n]` new · `[q]` close. The workspace (folders and conversations) is saved, and
  the start screen offers to restore it.

## Keys

| Where | Key | Does |
|---|---|---|
| anywhere | **Meta+C** | open / show / hide the deck |
| anywhere | **Meta+Shift+C** | jump to the session waiting longest |
| anywhere | **Meta+Alt+C** | open the island popup |
| deck | **F4** | new session in a new tab next to this one (picker; Enter keeps this folder) |
| deck | Shift+F4 | new tab, same folder, no picker |
| deck | **F5** / Shift+F5 | new session side by side (same folder / pick one) |
| deck | **F6** / Shift+F6 | new session below (same folder / pick one) |
| deck | **F7** | close the pane (asks first if Claude is working) |
| deck | Shift+F7 | move the pane out into its own tab |
| deck | **F8** | zoom the pane on/off |
| deck | **Ctrl+Shift+Space** | jump to the session waiting longest |
| deck | Ctrl+1 … Ctrl+9 | go to session N (the island's numbering) |
| deck | Ctrl+PgUp / Ctrl+PgDn | previous / next tab |
| deck | Ctrl+Shift+PgUp / PgDn | move the tab left / right |
| deck | Ctrl+Shift+O | session overview |
| deck | Alt+arrows, Alt+h/j/k/l | move between panes |
| deck | Alt+Shift+arrows | move the pane |
| deck | Ctrl+Alt+Shift+arrows | resize the pane |
| deck | Ctrl+Shift+L | cycle layouts |
| deck | Ctrl+Shift+G / Ctrl+Shift+U | tile all sessions in one tab / spread them back into tabs |
| deck | Ctrl+Shift+R | restore the last workspace |

Ctrl+Shift+N, Ctrl+Shift+Enter and Ctrl+Shift+- also work, the same as F4, F5 and F6.

## Install

Requirements: KDE Plasma 6 (Wayland or X11), kitty, a Rust toolchain, and Claude Code.

```sh
git clone https://github.com/borislisandro/claude-deck
cd claude-deck
packaging/install.sh --add-to-panel   # or without the flag, then add "Claude Deck" via Add Widgets
```

The installer builds the binary into `~/.local/bin`, installs the panel widget, a launcher
("Claude Deck") and the global shortcuts. With `--add-to-panel` it also places the island left of
your first system monitor, with a small spacer so Panel Colorizer gives it an island of its own. `packaging/install.sh --uninstall` removes everything.

## Use

```sh
claude-deck              # open the deck; from a project folder, a session there
claude-deck new ~/code/x --placement vsplit
claude-deck ls           # sessions and their states
claude-deck next-waiting --raise
claude-deck quit
```

`claude-deck --help` lists everything else (peek, tile, tail, digest, restore…).

## Configure

- **Panel look and behavior:** right-click the island, then *Configure Claude Deck…*. Options:
  colors, orange or monochrome critters, animations on/off, timers, peek on hover, thumbnail, and whether the island
  draws its own background (turn this off if Panel Colorizer already draws islands).
- **Daemon:** `~/.config/claude-deck/config.json`. Every key is optional:

  ```json
  {
    "notify": true,
    "remind_after_min": 5,
    "kdeconnect_after_min": 0,
    "sounds": false,
    "color_tabs": true,
    "claude_args": ["--model", "opus"],
    "project_roots": ["~/Documents/Github"]
  }
  ```
- **Extra kitty settings for the deck only:** `~/.config/claude-deck/kitty.conf`.

## How it works

```
 Plasma panel widget ──exec──▶ claude-deck CLI ──unix socket──▶ claude-deck daemon
   (QML island)        watch --once long-poll                     │  ▲
                                                                  │  │ hooks (claude-deck hook)
                       kitty @ (launch, focus, tail, kitten)      ▼  │ watcher.py (focus, close)
                                                              kitty deck ── claude sessions
                       KWin script / D-Bus (raise, highlight) ◀┘
```

- `claude-deck hook` is called by Claude for every hook event. It reads the event and passes a
  trimmed copy to the daemon over a 0600 socket in `$XDG_RUNTIME_DIR`. It prints nothing, since
  hook stdout can end up in Claude's context, always exits 0, and gives up after 200 ms.
- The daemon's state machine (`src/state.rs`) is pure and unit-tested against hook payloads.
- The panel widget never polls. It runs `claude-deck watch --once`, which blocks until something
  changes. While the deck is off, that call waits for the socket to appear.
- Raising the deck goes through a short-lived KWin script, so it works from the panel, from
  notifications and from shortcuts, despite Wayland focus-stealing prevention.

## Develop

```sh
cargo test
claude-deck daemon --demo     # fake sessions cycling through every state, no kitty needed
kpackagetool6 -t Plasma/Applet -u plasmoid/package && systemctl --user restart plasma-plasmashell
```

MIT licensed. Not affiliated with Anthropic.
