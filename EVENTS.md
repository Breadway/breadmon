# breadmon — bread event integration

breadmon is a standalone TUI monitor manager: it works exactly the same
with or without `breadd` running. When breadd *is* present, a successful
live apply publishes an event into the shared bread automation fabric.
See the parent `bread` repo's `Documentation.md` — specifically its
"Namespaces" and "Integrating a bread\* app" sections — for the general
convention this follows.

App id: **`mon`**. Transport: `bread-utils`'s `bread_client` module
(feature `bread-client`) — the TUI links it directly and emits from the
same process that ran `hyprctl eval`. Each `emit` is its own short-lived
connection (`BreadClient::emit` never blocks or errors the caller).

This event is about breadmon's own live apply (`hyprctl eval
'hl.monitor({...})'` on [BOS](https://git.breadway.dev/breadway/bos)-patched
Hyprland). After that apply succeeds, breadmon also writes
`~/.config/hypr/monitors.json` (the store shared with the `bos-settings`
Display panel). The event is **not** fired by Display itself — that GUI
only edits the JSON. Vanilla/upstream Hyprland has no `eval` request
and no `hl.monitor()`, so apply fails there and this event is not
published.

## Events published (`bread.mon.*`)

| Event | Data | When |
|-------|------|------|
| `bread.mon.applied` | `{ "profile": <string or null> }` | After `hyprctl eval 'hl.monitor({...})'` succeeds. `profile` is the named snapshot that was just applied (the last loaded or saved profile this session, if the layout was not edited after that), or `null` for an ad-hoc layout. Not emitted when apply fails. |

## Commands honored (`bread.command.mon.*`)

None. breadmon is an interactive TUI, not a long-running daemon — a
command subscription would only be live while the TUI is open, which is
a poor control surface. Apply, load, and save stay keyboard-driven.
If/when breadmon grows a headless apply path, the corresponding
`bread.command.mon.apply` verb should be added at the same time, not
stubbed out ahead of it.

## Fail-safe behavior

- If breadd isn't installed or isn't running, `emit` is a silent no-op
  (`BreadClient::emit` never blocks or errors the caller) — breadmon's
  actual apply / profile / TUI functionality is entirely unaffected.
- There is no command subscription, so a breadd restart has nothing to
  reconnect.
