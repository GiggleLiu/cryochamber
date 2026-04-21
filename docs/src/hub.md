# Cryohub

`cryohub` runs a directory-scoped web dashboard on `http://127.0.0.1:8765` by default.

## Workspace layout

`cryohub` always operates on the current directory. `cd` into a directory whose immediate subdirectories are chambers (each subdirectory has its own `cryo.toml`):

```
~/my-chambers/
  chess-by-mail/     # cryo.toml + plan.md here
  mr-lazy/
  reports/
```

Then start the hub from that directory:

```bash
cd ~/my-chambers
cryohub start              # installs a service that survives reboot
cryohub start --foreground # run in foreground (no service)
cryohub stop               # stop and remove the service for this dir
cryohub status             # show whether a service is installed for this dir
```

`cryohub` rejects starting from a chamber dir (one with `cryo.toml`) — `cd` to the parent.

`cryohub status` and `cryohub stop` always also list any **other** cryohub services installed elsewhere on the machine, so you can find services started from a different cwd.

## What the UI does

- **Sidebar** — every chamber, sorted by running → stopped → external. Shows status dot, name, unread-message badge.
- **Main pane** — full detail for the selected chamber: status, task, next wake, notes, message history, log tail, send widget.
- **Lifecycle buttons** — `start` / `stop` / `restart` for chambers under the hub's cwd. External chambers show no lifecycle buttons.

## External chambers

Running daemons anywhere on the machine (registered via `cryo start` from any working directory) appear as **external** chambers if they aren't under the hub's cwd. They're monitor-only from the UI.

## Single-chamber layout

If you only have one chamber, `cd` to a parent directory and symlink the chamber into it:

```bash
mkdir -p ~/cryo-chambers
ln -s $(pwd) ~/cryo-chambers/my-chamber
cd ~/cryo-chambers && cryohub start
```

## Security

The default bind is `127.0.0.1`. If you pass `--host 0.0.0.0`, cryohub prints a warning because lifecycle actions are exposed over the network without authentication. Don't do that on a shared network. Token auth is tracked as future work.
