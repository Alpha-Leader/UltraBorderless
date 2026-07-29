## ultraborderless <img src="assets/icon.ico" alt="ultraborderless icon" width="48" height="48" align="left">
![Windows Only](https://img.shields.io/badge/platform-Windows-blue?logo=windows)
[![License](https://img.shields.io/badge/license-GPL--3.0-green)](LICENSE)
[![Downloads](https://img.shields.io/github/downloads/Alpha-Leader/ultraborderless/total)](https://github.com/Alpha-Leader/ultraborderless/releases)
[![Issues](https://img.shields.io/github/issues/Alpha-Leader/ultraborderless)](https://github.com/Alpha-Leader/ultraborderless/issues)

A lightweight Windows utility that strips the borders from any window and places it into a
**region** of your monitor — sized and anchored how you want, rather than only stretching it across
the whole display.

Built for ultrawides. On a 7680×2160 monitor you can run a game as a 3840×2160 borderless window
dead center, covering the taskbar, with 1920 px of desktop still usable on each side.

### Credit

This project is a fork of [**ihateborders** by Z1xus](https://github.com/Z1xus/ihateborders),
forked at commit `b5bb27e` (v1.1.1, 2026-02-16). All of the original window-manipulation work is
theirs — this fork adds region placement, always-on-top, persisted settings, and a number of
correctness fixes.

See [NOTICE](NOTICE) for the full list of modifications, and go star
[the original](https://github.com/Z1xus/ihateborders).

### Installation

Download the latest release from the [Releases](https://github.com/Alpha-Leader/ultraborderless/releases)
page. It's a single self-contained `.exe` — no installer, no dependencies.

### Usage

1. Run the executable.
2. Pick a window from the dropdown.
3. Choose a **Placement** mode.
4. Click **Make Borderless**. Click **Restore Borders** to put it back exactly as it was.

#### Placement modes

| Mode | Behavior |
| --- | --- |
| **Region** | Places the window into a sub-rectangle of the display. Set a size, pick an anchor. |
| **Full display** | Fills the entire monitor. |
| **Leave in place** | Strips borders without moving or resizing. |

In Region mode you get size fields with `4K` / `1440p` / `1080p` presets, and an anchor of
**Centered**, **Left**, **Right**, or **Custom** (explicit x/y offset). A live preview shows exactly
where the window will land before you commit — e.g. `→ 3840x2160 at (1920, 0)` with
`sides free: 1920px | 1920px`. It turns amber if the region runs past the display edge.

#### Always on top

**This is what makes taskbar coverage work.** Windows only auto-hides the taskbar for a window that
fills an *entire* monitor, so a narrower region can never cover it by position alone. Enabling this
puts the window in the topmost band instead. It's on by default; restoring borders returns the
window to its original z-order.

### Interface

- **[B]** — borderless window
- **[W]** — windowed (has borders)
- System windows are filtered out, and the list refreshes every 5 seconds
- The display dropdown only appears if you have more than one monitor

### Keyboard shortcuts

- `F5` — refresh the window list
- `Esc` — clear the current selection

### Settings

Persisted to `%APPDATA%\ultraborderless\config.txt` as plain `key=value` text. Saved on exit and
after each action; delete the file to reset to defaults.

### Using it with games

- Set the game to **Windowed** mode first, not its own borderless or exclusive-fullscreen mode —
  exclusive fullscreen has no border to strip.
- Set the game's internal render resolution to match the region size.
- Some games re-assert their own window size after placement or on a focus/resolution change, and a
  few ignore external placement entirely.
- Restore a window before quitting: saved original frames are held in memory only and do not
  survive a restart of this app.

### Requirements

- Windows 10/11
- To modify an elevated window (Task Manager, an admin terminal), run this as administrator too —
  Windows blocks a normal-privilege process from restyling an elevated one.

### Building

```bash
git clone https://github.com/Alpha-Leader/ultraborderless
cd ultraborderless
cargo build --release
```

The binary lands in `target/release/ultraborderless.exe`.

Note that `.cargo/config.toml` sets `target-cpu=native`, so a locally built binary is tuned to the
machine that built it and is not portable to older CPUs.

Run the tests with `cargo test`.

### License

GPL-3.0-only — see [LICENSE](LICENSE). As a derivative of ihateborders, which is GPL-3.0, this
project remains under the same license.
