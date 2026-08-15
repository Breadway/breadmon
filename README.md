# breadmon

A terminal UI monitor manager for Hyprland. Lets you position, configure, and mirror displays interactively, then apply a live layout on [BOS](https://git.breadway.dev/breadway/bos)-patched Hyprland.

The Display panel in `bos-settings` (GUI) and breadmon (TUI) share `~/.config/hypr/monitors.json` — the layout Hyprland reads at login/reload. Applying in breadmon writes that file so the settings app and the next session stay in sync. Named profiles remain optional snapshots under `~/.config/breadmon/profiles/`.

## Requirements

- **[BOS (Bread OS)](https://git.breadway.dev/breadway/bos)'s patched Hyprland build** for live apply. The `a` key runs `hyprctl eval 'hl.monitor({...})'` — a BOS-specific Lua extension. Vanilla/upstream Hyprland has no `eval` request and no `hl.monitor()`, so apply will fail there with an explicit error instead of the raw hyprctl response. Viewing, arranging, and saving profiles work on any Hyprland; only live apply needs BOS.
- The `hyprctl` binary must be on `PATH`
- Rust toolchain (to build from source)

## Install

Via [bakery](https://git.breadway.dev/Breadway/bread-ecosystem), the bread ecosystem package manager:

```
bakery install breadmon
```

Or build from source:

```
cargo build --release
```

The binary is written to `target/release/breadmon`.

## Usage

```
breadmon
```

The TUI opens with four tabs, switchable by number key or mouse click:

| Key | Tab |
|-----|-----|
| `1` / F1 | Layout |
| `2` / F2 | Config |
| `3` / F3 | Mirror |
| `4` / F4 | Profiles |

### Layout

A scaled canvas showing all connected monitors. The selected monitor is highlighted.

| Key | Action |
|-----|--------|
| `hjkl` / arrow keys | Move selected monitor 1 px |
| `Shift+hjkl` | Move selected monitor 10 px |
| `Tab` / `n` | Cycle to next monitor |
| `Shift+Tab` / `p` | Cycle to previous monitor |
| `0` | Auto-arrange monitors left-to-right with no gaps |
| `[` / `]` | Zoom canvas out / in |
| Mouse drag | Drag monitor to reposition |

Monitors snap to each other's edges when dragged or nudged within 10 px.

### Config

Per-monitor settings for the currently selected display.

| Key | Action |
|-----|--------|
| `jk` / `Tab` | Move between fields |
| `hl` / scroll | Cycle field value |
| `,` / `.` | Scale −0.1 / +0.1 |
| `[` / `]` | Switch to previous / next monitor |
| `Enter` | Commit scale edit and apply |
| `Esc` | Discard pending edits |

Fields: Resolution, Refresh rate, Scale, Transform (rotation/flip), VRR, DPMS, Mirror of.

The header shows the monitor's PPI and a suggested fractional scale when physical dimensions are available.

### Mirror

Finds the best common mode between two monitors and sets one to mirror the other. Use `Tab` to move focus between source and target, `hl` to cycle the selection, `Enter` to confirm.

### Profiles

Named snapshots of the current monitor configuration, stored as TOML files. Loading a profile updates the TUI; applying it (`a`) also writes `monitors.json`.

| Key | Action |
|-----|--------|
| `jk` | Navigate profile list |
| `Enter` | Load selected profile |
| `d d` | Delete selected profile (double-press within 3 s) |
| `Tab` | Move focus to name input / save button |

Profiles are saved to `~/.config/breadmon/profiles/`.

### Global keys

| Key | Action |
|-----|--------|
| `a` | Apply current configuration via `hyprctl eval 'hl.monitor({...})'` (BOS-patched Hyprland only) and write `~/.config/hypr/monitors.json` |
| `s` | Write `~/.config/hypr/monitors.json` without a live apply |
| `r` | Refresh monitor list from Hyprland |
| `Ctrl+Z` | Undo last change (up to 20 steps) |
| `q` / `Ctrl+C` | Quit (prompts once if there are unsaved changes) |

breadmon also listens on Hyprland's event socket and reloads the monitor list automatically when a display is connected or disconnected.

## Config

**Shared store:** `~/.config/hypr/monitors.json` — the same file the bos-settings Display panel edits and Hyprland applies on login/reload. breadmon is the TUI; Display is the GUI. Schema:

```json
{
  "monitors": [
    { "output": "", "mode": "preferred", "position": "auto", "scale": "auto", "mirror": "<optional string>" }
  ]
}
```

Empty `output` is the wildcard default (any connector). breadmon loads this file on start (overlaid onto the live `hyprctl` list) and writes it — pretty JSON — after a successful apply, and when you press `s`.

**Named snapshots:** plain TOML under `~/.config/breadmon/profiles/`. Each file records the monitor name, mode, position, scale, transform, VRR, DPMS, and mirror source. They are created and managed through the Profiles tab. Applying a profile writes `monitors.json` so Hyprland and Display stay in sync.

## bread event integration

breadmon works the same with or without `breadd`. After a successful live apply (`hyprctl eval 'hl.monitor({...})'` on BOS-patched Hyprland — not the `bos-settings` Display panel), it publishes `bread.mon.applied`. If breadd is down, the emit is a silent no-op; apply itself is unchanged. See [EVENTS.md](EVENTS.md) for the bus contract. `bread` is not a bakery dependency.
