# ✻ claude-deck

**Run many Claude Code sessions in one terminal deck, and see what each of them needs right on
your KDE Plasma panel.**

![The Claude Deck island on a Plasma panel: one session working, one waiting on a permission, one finished four minutes ago](docs/island.svg)

You start a few Claude Code sessions in different repos and switch to something else. One is
waiting on a permission prompt, one finished ten minutes ago, and one is still working, but
nothing tells you which is which. If you were in a fullscreen game, Plasma even held back the
notifications.

claude-deck fixes that with two pieces that work together:

- **The deck:** one [kitty](https://sw.kovidgoyal.net/kitty/) window holding all your Claude Code
  sessions in tabs and splits. Each tab title shows its sessions' state, and each session knows
  its model, branch and last action.
- **The island:** a widget on your Plasma panel with one pixel critter per session. The critter
  runs while Claude works, thinks when Claude needs you, and lies down to sleep when it's your
  turn. Hover to peek at the session, click to jump to it.

The state comes from Claude Code's own hooks, not from scraping the screen, and your own
`~/.claude/settings.json` is never touched.

---

- [What you see](#what-you-see)
- [How you use it](#how-you-use-it)
- [The panel integration in detail](#the-panel-integration-in-detail)
- [Install](#install)
- [Keys](#keys)
- [Command line](#command-line)
- [Configure](#configure)
- [How it works](#how-it-works)
- [Troubleshooting](#troubleshooting)
- [Develop and test](#develop-and-test)

## What you see

### The critters

![The four moods: a running critter for working, a thinking one with a thought bubble when Claude needs you, a sleeping one when it's your turn, and a grey one when Claude exited](docs/moods.svg)

| State | Critter | Light | Means |
|---|---|---|---|
| **Working** (green) | runs, kicking up dust | breathes | Claude is thinking or running tools. Nothing to do. |
| **Needs you** (yellow) | hand on chin, thought bubble | blinks, quick ripple | A permission prompt, a question (`AskUserQuestion`), a plan to approve, or an MCP server asking something. |
| **Your turn** (red) | asleep, Z's floating up | slow ripple until you've looked | The turn is over and Claude waits for your next prompt. Once you've looked, the light dims and stops rippling. |
| **Exited** (grey) | grey, eyes shut | hollow ring | Claude quit or crashed. The pane stays open with a menu. |

A session that has waited unseen for over a minute shows its wait time beside the light (`4m`).
A turn that ended on an API error (rate limit, overload) gets a small red `!`.

The island always follows the deck's order: tabs left to right, and panes within a tab in reading
order. Move a tab and the island follows. With more than 5 sessions (configurable) the island
shows one critter per tab: the tab's most urgent session, with a count badge. With more than 5
tabs as well, it shows one count per state.

### The deck

Inside the deck, each kitty tab title carries the same information:

```
 ◆● 2 JobScraper +1 · Sonnet 5.5 4m │ ● 3 claude-deck · Opus 5.5 │ ○ 4 rv32-core
 │└ the other session in the tab     │                            └ seen: dimmed
 └ the most urgent session leads: needs you, waiting 4 minutes
```

Tabs whose session waits on you also get a tinted background. When a session ends, its pane
doesn't vanish:

```
────────────────────────────────────────
  Claude exited with code 1.

  [r] or Enter resume this conversation   [n] start a new one   [q] close the pane
```

Only `q` closes. Enter resumes, so a habitual keypress right after a crash can't throw the
session away.

## How you use it

**Start the deck.** Press **Meta+C**, or run `claude-deck`. From a project folder this opens a
session there; from anywhere else it opens the project picker, which lists the folders you've
used Claude in (most recent first) and the ones under your project roots. Type to filter, or type
a path. If you had a deck open before, the picker also offers to restore it.

**Add sessions.** **F4** opens a new tab with the picker (Enter keeps the current folder),
**F5** a session beside this one, **F6** one below. The deck is kitty with your own
`kitty.conf` underneath, so your font, theme and keys carry over.

**Go do something else.** Write code in your editor, review a PR, play a game. The island keeps
watch:

- A critter turns **yellow**: Claude is blocked on you. You also get a notification with an
  **Open** button (see [Notifications](#notifications) for when you do and don't).
- A critter falls **asleep**: that session finished. Hover it to read the end of its last message
  without leaving what you're doing, and click to jump there.
- **Meta+Shift+C**, from anywhere, jumps to whichever session has waited longest, needs-you
  first.

**Back from fullscreen.** Plasma holds notifications while a fullscreen app runs. When you leave
it, the island sends one "While you were away: 2 sessions are waiting" notification and opens
its popup.

**Rearrange.** In the popup, drag a session onto another tab: a map of that tab's panes appears
and shows which half of which pane it will take. Drop it on the "own tab" strip to move it out
again. In the deck, **Ctrl+Shift+G** tiles every session into one tab, and **Ctrl+Shift+U**
spreads them back out.

**Close for the day.** Quit the deck (right-click the island, *Quit*, or `claude-deck quit`).
The workspace, meaning which folders and which conversations in which tabs, is saved, and the
picker or **Ctrl+Shift+R** brings it back, each session resuming its conversation.

Some ways people use it:

- **Parallel agents across repos:** a backend fix, a frontend feature and a docs pass, each in
  its own tab. You answer whichever lights up yellow first.
- **Long tasks in the background:** start a big refactor or test run, then keep working
  elsewhere. A red, sleeping critter means it's done.
- **One repo, several angles:** split a tab into a session that implements and one that reviews,
  both visible at once.

## The panel integration in detail

The island is a Plasma 6 widget (`com.borislisandro.claudedeck`). It only takes space while the
deck runs; otherwise it is zero pixels wide.

| On the island | Does |
|---|---|
| **Hover a critter** | After a short pause, kitty switches to that session's tab without taking focus, and the deck window is highlighted (the same effect the task manager uses). A card shows project, branch, model, state, last action, last prompt and the end of Claude's latest message. In the popup, hovering only highlights: a session opens when you click it. |
| **Click a critter** | Focuses that pane and raises the deck. Clicking the session you're already in hides the deck. |
| **Middle-click** | New session (with the picker). |
| **Scroll** | Switch to the next or previous session. |
| **Click the chevron** | The popup. It sits at one end of the pill (right by default), so a click that just misses a critter never opens it. |
| **Right-click** | New session, show sessions, jump to next waiting, tile, spread into tabs, restore, quit. |
| **Meta+Alt+C** | Opens the popup (set by `install.sh --add-to-panel`; change it in the widget's settings). |

**The popup** inflates out of the panel and lists your sessions grouped the way the deck has
them. Everything starts folded, and the popup is exactly as tall as what it shows:

- A tab with one session is a card: state, wait time, what it's doing, branch, model.
- A tab with several sessions is a header (state counts, the shared model, spread into tabs,
  close tab); click it to list its sessions, one line each.
- Click a session anywhere to open it in the deck; drag it onto another tab to tile it there.
- Its arrow opens it up: Claude's latest message in full (scroll to read it all; nothing until
  Claude has said something), and a box to prompt that session from the popup. Enter sends,
  Shift+Enter is a new line, and while Claude works the message is queued. The box is off while
  Claude is asking you something, since typing would answer that.
- Close buttons ask inline ("Close?") when Claude is busy, and the popup stays open, so you can
  close several in a row.

**Attention:** while anything waits unseen, the widget reports *needs attention* to Plasma, so
the pill gets a colored border and a slow glow.

### Notifications

The deck tries to tell you what you need to know once, and then leave you alone.

| When | What you get |
|---|---|
| A session **needs you** and you're not in the deck | A notification that stays until handled: *"api needs you"*, then what it wants (`Wants to: Running make deploy`, `Asking: Postgres or SQLite?`, `Plan ready for your review`) and the prompt it was working on. |
| A session **finishes** and you're not in the deck | *"api is done"* and the first line of what Claude said. It times out after 15 s and stays in Plasma's history. |
| A turn **ends on an error** | *"api stopped on an error"* with the error. |
| Claude **dies while busy or waiting on you**, and you weren't looking | *"api: Claude exited unexpectedly"*. Closing a pane or quitting the deck doesn't count. |
| A session has been **blocked on you** for `remind_after_min` | One reminder. Finished sessions get none: they lose nothing by waiting. |
| You come back from a **fullscreen app** | One summary of what began waiting meanwhile. Not for what has been red since this morning, and not when the deck itself was the fullscreen window. |
| A **usage limit** you had nearly used resets | One notification (see [Usage limits](#usage-limits)). |

What you *don't* get:

- **Notifications while you're in the deck** (its window has focus): the tab bar and the island
  already show it, and the pane is right there. Hovering the island doesn't count as being in the
  deck.
- **Stacks.** A session has one notification slot, so a new state replaces the old one, and a
  summary replaces the previous summary. Two sessions with the same project name are told apart
  by branch (`api (feat/x)`).
- **Stale ones.** Looking at a session, answering its prompt, or its moving on withdraws its
  notification, even if the answer arrives while the notification is still being drawn. A new
  daemon (after `upgrade`, or a crash) takes down what the old one left behind and shows the
  ones still owed.
- **Chime bursts.** Sounds closer than 2.5 s apart play once.

While a prompt is open, other work doesn't clear it: a background subagent finishing tool calls
leaves your "needs you" yellow until you've answered it.

**Raising the deck** goes through a short-lived KWin script, so it works from the panel, from a
notification and from a global shortcut, despite Wayland's focus-stealing prevention.

**Panel Colorizer:** if you use it to draw panel islands, turn off *Draw the island background*
in the widget settings (`install.sh --add-to-panel` does this for you, and adds a spacer so the
critters get an island of their own).

## Install

You need:

- KDE Plasma 6 (Wayland or X11)
- kitty
- Claude Code (`claude` on your PATH, or in `~/.local/bin` or `~/.claude/local`)
- a Rust toolchain (1.85 or newer)
- `gdbus` (part of GLib)
- optional: `pw-play` (PipeWire) for sounds, `kdeconnect-cli` for phone pushes

```sh
git clone https://github.com/borislisandro/claude-deck
cd claude-deck
packaging/install.sh --add-to-panel   # or without the flag, then add "Claude Deck" via Add Widgets
claude-deck doctor                    # checks that everything is in place
```

The installer builds the binary into `~/.local/bin`, then installs the panel widget, a launcher
("Claude Deck", with jump lists for *New Session*, *Restore* and *Quit*) and the global shortcuts.
With `--add-to-panel` it also places the island left of your first system monitor widget on the
top panel.

Reinstalling never closes your sessions: the installer runs `claude-deck upgrade`, which hands
the running deck over to the new binary. `packaging/install.sh --uninstall` removes everything
except your saved workspace in `~/.local/state/claude-deck`. Remove the widget from your panel
first, or Plasma will show a placeholder where it was.

## Keys

| Where | Key | Does |
|---|---|---|
| anywhere | **Meta+C** | open / show / hide the deck |
| anywhere | **Meta+Shift+C** | jump to the session waiting longest |
| anywhere | **Meta+Alt+C** | open the island's popup |
| deck | **F4** | new session in a new tab next to this one (picker; Enter keeps this folder) |
| deck | Shift+F4 | new tab, same folder, no picker |
| deck | **F5** / Shift+F5 | new session side by side (same folder / pick one) |
| deck | **F6** / Shift+F6 | new session below (same folder / pick one) |
| deck | **F7** | close the pane (asks first; Enter confirms, Esc keeps it) |
| deck | Shift+F7 | move the pane out into its own tab |
| deck | **F8** | zoom the pane on/off |
| deck | **Ctrl+Shift+Space** | jump to the session waiting longest |
| deck | Ctrl+1 … Ctrl+9 | go to session N (the island's numbering) |
| deck | **Alt+Left / Alt+Right**, Ctrl+PgUp / Ctrl+PgDn | previous / next tab |
| deck | Ctrl+Shift+PgUp / PgDn | move the tab left / right (the island follows) |
| deck | Ctrl+Shift+O | session overview (Enter or 1–9 jumps) |
| deck | Alt+Up / Alt+Down, Alt+h/j/k/l | move between panes |
| deck | Alt+Shift+arrows | move the pane |
| deck | Ctrl+Alt+Shift+arrows | resize the pane |
| deck | Ctrl+Shift+L | cycle layouts (splits, tall, grid, stack) |
| deck | Ctrl+Shift+G / Ctrl+Shift+U | tile every session in one tab / spread every tab back out |
| deck | Ctrl+Shift+R | restore the last workspace |

Ctrl+Shift+N, Ctrl+Shift+Enter, Ctrl+Shift+- and Ctrl+Shift+W do the same as F4, F5, F6 and F7.
Ctrl+Shift+Alt+Enter is Shift+F5. Add your own deck-only keys in
`~/.config/claude-deck/kitty.conf`.

## Command line

```sh
claude-deck                    # open the deck; from a project folder, a session there
claude-deck ls                 # sessions and their states
claude-deck doctor             # check kitty, claude, the config, the daemon, the widget
claude-deck new ~/code/x --placement vsplit    # tab | vsplit | hsplit | window; --pick for the picker
claude-deck next-waiting --raise
claude-deck tail <sid> --lines 20              # the last lines of a session's pane
claude-deck reply <sid>                        # Claude's latest message in a session
claude-deck send <sid> run the tests            # type a prompt into a session and submit it
claude-deck place <sid> --next-to <kitty window> --side left   # no --next-to: its own tab
claude-deck panes              # every tab's panes and where they sit, as JSON
claude-deck watch              # the full state as JSON, a line per change
claude-deck restore            # reopen the saved workspace (skips what is already open)
claude-deck upgrade            # swap in a newly built binary; the deck and its sessions stay
claude-deck quit               # save the workspace and close the deck
```

`claude-deck --help` lists everything else (activate, goto, peek, ack, tile, untile, close,
close-tab, interrupt, digest, overview…). `watch` and `ls` make it easy to script around the
deck, for example to feed your own status bar.

## Configure

**The island:** right-click it, then *Configure Claude Deck…*

| Setting | Default |
|---|---|
| Hovering a session on the island shows its pane | on |
| After a fullscreen app closes, open the popup if sessions are waiting | on |
| Show how long a session has waited | on |
| Animate the critters | on |
| Claude-orange critters (off: the panel's text color) | on |
| Draw the island background (off: let Panel Colorizer style it) | on |
| Session list button | right end |
| Collapse above N sessions | 5 |
| Colors for working / needs you / your turn / exited | green / amber / red / grey |
| Path to the `claude-deck` binary | `~/.local/bin/claude-deck` |

**The daemon:** `~/.config/claude-deck/config.json`. Every key is optional; the daemon reads it
at start. `claude-deck doctor` tells you if the file doesn't parse; the deck then uses the
defaults and says so in its log.

```json
{
  "notify": true,
  "remind_after_min": 5,
  "kdeconnect_after_min": 0,
  "sounds": false,
  "sound_needs_you": "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga",
  "sound_your_turn": "/usr/share/sounds/freedesktop/stereo/complete.oga",
  "notify_usage_reset": true,
  "usage_reset_min_percent": 80,
  "sound_usage_reset": "/usr/share/sounds/freedesktop/stereo/service-login.oga",
  "color_tabs": true,
  "claude_args": ["--model", "opus"],
  "project_roots": ["~/Documents/Github"]
}
```

| Key | Does |
|---|---|
| `notify` | Desktop notifications when a session turns yellow or red while you're away from the deck. |
| `remind_after_min` | Remind once more when a session has been blocked on you this many minutes (0: never). Not sent while you're in the deck, and not for finished sessions. |
| `kdeconnect_after_min` | Ping your phone through KDE Connect after this many minutes unseen, finished or blocked (0: never). Not while you're in the deck. |
| `sounds`, `sound_needs_you`, `sound_your_turn` | Play a sound (through `pw-play`) on those transitions. |
| `notify_usage_reset`, `usage_reset_min_percent`, `sound_usage_reset` | Say so when your 5-hour or weekly usage limit resets, but only if that window was at least this full (percent) when it did. Uses `notify`, plays the sound when `sounds` is on, and pings your phone when `kdeconnect_after_min` is set. See [Usage limits](#usage-limits). |
| `color_tabs` | Tint the kitty tab of a session waiting on you. |
| `claude_args` | Extra arguments for every `claude` the deck starts. |
| `project_roots` | Folders whose subfolders the picker lists. |

### Usage limits

Claude Code tells only its status line how full your 5-hour and weekly limits are and when
they reset (Pro, Max and Ultra plans). Deck sessions therefore run with `claude-deck statusline`
as their status line: it hands the numbers to the daemon, and shows `5h 23% (2h 10m) · 7d 41% (3d 2h)`
under the prompt. When a window's reset time passes, the daemon tells you, with a notification, a
sound, and a phone ping if you enabled those. The daemon keeps its own clock, so it
tells you even after every session is closed.

- If `~/.claude/settings.json` has a status line of your own, it stays the one that is shown
  (the deck only listens in). A status line set in a *project's* settings is replaced in deck sessions.
- A status line makes Claude hide `esc to interrupt` from its footer; the deck recognises Claude's
  spinner line instead, so cancelled prompts are still caught.
- Nothing is announced for windows that were barely used, or that reset while the computer slept.

**Kitty, for the deck only:** `~/.config/claude-deck/kitty.conf` is read after the deck's own
settings, so it can override them.

## How it works

```mermaid
flowchart LR
    subgraph kitty["kitty deck (one OS window)"]
        RS["run-session<br/>(one per pane)"] --> CL["claude --settings (deck hooks)"]
        W["watcher.py<br/>focus · close · pane order"]
        TB["tab_bar.py<br/>tab titles"]
        K["deck.py kitten<br/>peek · tile · place · close"]
    end
    CL -- "claude-deck hook<br/>(every hook event)" --> D
    RS -- "register · exited" --> D
    W -- "unix socket" --> D
    D[("claude-deck daemon<br/>state machine")] -- "kitty @ (remote control)" --> K
    D -- "state.json" --> TB
    D -- "KWin script · D-Bus" --> KW["KWin<br/>raise · highlight"]
    D -- "notifications" --> N["Plasma notifications<br/>· KDE Connect"]
    P["Plasma widget<br/>(the island)"] -- "claude-deck watch --once<br/>(long poll)" --> D
    P -- "activate · peek · place · close…" --> D
```

- **Every deck pane runs `claude-deck run-session`**, which starts
  `claude --settings <runtime>/settings.json`. Claude layers that file on top of your own
  settings, so every hook event also runs `claude-deck hook`. Your `~/.claude/settings.json`
  is never modified.
- **`claude-deck hook`** reads the event, keeps only what the deck shows (tool inputs can be
  megabytes), and hands it to the daemon over a `0600` unix socket in `$XDG_RUNTIME_DIR`. It
  prints nothing, because some hooks' output reaches Claude's context. It always exits 0 and gives
  up after 200 ms. It waits for the daemon to apply the event before exiting, so events arrive in
  the order Claude fired them.
- **The daemon** (`src/daemon.rs`) owns the session store and the deck's kitty process. Its state
  machine (`src/state.rs`) is pure, with no I/O and no clock, and is unit-tested against hook
  payloads:

  ```mermaid
  stateDiagram-v2
      direction LR
      [*] --> YourTurn: pane opens
      YourTurn --> Working: UserPromptSubmit
      Working --> NeedsYou: PermissionRequest · AskUserQuestion · ExitPlanMode · Elicitation
      NeedsYou --> Working: PostToolUse · PermissionDenied · ElicitationResult
      Working --> YourTurn: Stop · StopFailure · Esc (interrupt)
      YourTurn --> Exited: SessionEnd · process exit
      Working --> Exited: process exit
      Exited --> YourTurn: resume or new, from the pane's menu
  ```

  Whether you've *seen* a waiting state is tracked separately: it counts as seen if that pane had
  keyboard focus when the state arrived, or once you focus it, click it, or mark it seen.
  Hovering (peeking) doesn't count.
- **Esc has no hook.** Claude fires nothing when you interrupt it, so the deck notices on its own:
  from a `[Request interrupted by user]` record in the transcript, or from Claude's
  "esc to interrupt" footer disappearing for two looks in a row. The second signal also catches
  cancelling before Claude replied. The footer check only trusts the footer's absence once it has
  seen the footer at least once, so a Claude version that words it differently can't turn busy
  sessions red. Esc at a permission prompt or a question is caught from the transcript too.
- **Focus is checked, not just reported.** The watcher's focus reports are fire-and-forget, so a
  lost one could leave the daemon thinking you're still in the deck (and silencing
  notifications). While focus matters, the daemon asks kitty itself every few seconds and
  corrects a disagreement that persists.
- **The model** comes from the `SessionStart` hook, then from each reply in the transcript, so a
  `/model` switch shows up. Before the first reply it is read from Claude's startup banner.
- **The widget never polls.** It runs `claude-deck watch --once --since <rev>`, which blocks
  until something changes (or 25 s pass). While the deck is off, that call waits for it to start.
  Critters and cards are updated in place, so animations and an open hover card survive updates.
- **Inside kitty,** `watcher.py` reports focus changes, closed panes and the on-screen pane order;
  `tab_bar.py` draws the tab titles from `state.json`; the `deck.py` kitten does what needs kitty's
  internals (peeking without stealing focus, tiling as an even grid of splits, placing a pane on
  one side of another, closing with a confirmation).

**Files**

| Path | What |
|---|---|
| `$XDG_RUNTIME_DIR/claude-deck/` | `daemon.sock`, `daemon.lock`, `kitty.sock`, the hooks' `settings.json`, `state.json` (0700) |
| `~/.local/share/claude-deck/kitty/` | the deck's kitty config, watcher, tab bar and kitten, rewritten from the binary at every start |
| `~/.local/state/claude-deck/` | `workspace.json` (the saved workspace) and `daemon.log` |
| `~/.config/claude-deck/` | your `config.json` and deck-only `kitty.conf` |

**Robustness:**

- One daemon per user: a lock file makes a second one (say, Meta+C pressed twice) back off.
- The workspace and state files are written atomically.
- `upgrade` swaps the daemon under a running deck and keeps the island's order.
- Restoring skips conversations that are already open, so a conversation never ends up in two
  panes.
- Helper processes are reaped, so the daemon doesn't collect zombies over a long day.
- A panic in one background task can't wedge the daemon: its locks tolerate poisoning, and each
  loop (reminders, focus, sync, interrupt detection...) is restarted if it dies.
- Hooks from a conversation that has ended, from a subagent while a prompt is open, or without a
  session id can't revive, clear or invent a session.

## Troubleshooting

Start with **`claude-deck doctor`**. It checks kitty, `claude`, your config file, the daemon, the
hook settings, `gdbus`, the widget and your PATH, and tells you what's missing.

| Symptom | Try |
|---|---|
| The island isn't on the panel | It only shows while the deck runs: press Meta+C. Otherwise add it via *Add Widgets → Claude Deck*. |
| Meta+C does nothing | Run `packaging/install.sh` again (it registers the shortcuts), or set them in *System Settings → Shortcuts → Claude Deck*. |
| A session stays green after it finished | Was it started outside the deck? Only deck panes report hooks. Check `~/.local/state/claude-deck/daemon.log`. |
| Panel shows nothing but the deck runs | The widget can't find the binary: set its path in the widget's settings. |
| Notifications appear late | Plasma holds them in fullscreen; you'll get the "While you were away" summary when you leave it. |
| You edited the deck's kitty config and it was reset | It's generated; put your changes in `~/.config/claude-deck/kitty.conf`. |
| Something looks stuck | `claude-deck upgrade` restarts the daemon without closing anything. |

## Develop and test

```sh
cargo test                                      # unit tests, plus tests/cli.rs (below)
cargo clippy --all-targets -- -D warnings
python3 -m unittest discover -s tests/python    # the Python that runs inside kitty
node --test plasmoid/tests/*.test.mjs           # the widget's logic
claude-deck daemon --demo                       # fake sessions cycling through every state, no kitty
kpackagetool6 -t Plasma/Applet -u plasmoid/package && systemctl --user restart plasma-plasmashell
python3 docs/make-art.py                        # redraw the README's pictures from the sprites
```

What the tests cover:

- **`src/**` unit tests:** the state machine event by event (permissions, questions, plans,
  elicitations, compaction, `/clear`, errors, exits, resumes, interrupts, peeks), model names
  including Bedrock and Vertex ids, tool descriptions, the wire protocol (including the exact
  messages the kitty watcher sends), tab tints, restore, the interrupt detector, transcripts,
  git branches and worktrees, the picker, config parsing, the exit menu, the single-instance
  lock, and zombie-free helpers.
- **`tests/cli.rs`:** a real daemon in `--demo` mode, driven through the real CLI in a private
  runtime/config folder. Hooks go in through `claude-deck hook`, and the test checks they show up
  in order in `watch`, `ls` and the tab bar's state file. It also covers the long poll, error
  reporting, a second daemon backing off, `quit` cleaning up, and `doctor`.
- **`tests/python`:** `deck.py`, `watcher.py` and `tab_bar.py` against fake kitty objects.
- **`plasmoid/tests`:** grouping, urgency, the drop map's hit testing, the island's collapse,
  scrolling, parsing `watch` output.

The widget is a QML Plasma applet in `plasmoid/package/contents/ui`. Its pure logic lives in
`logic.js` so it can be tested with node. To try QML changes without touching your panel, run it
offscreen against a demo daemon:

```sh
export XDG_RUNTIME_DIR=$(mktemp -d)                      # a private runtime dir: a separate deck
claude-deck daemon --demo &
QT_QPA_PLATFORM=offscreen QT_FORCE_STDERR_LOGGING=1 plasmawindowed com.borislisandro.claudedeck
```

MIT licensed. Not affiliated with Anthropic.
