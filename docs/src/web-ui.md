# Web UI

`cryo web` runs a workspace-scoped web dashboard on `http://127.0.0.1:8765` by default.

## Workspace layout

A workspace is a directory containing a `chambers/` subdirectory. Each chamber is a regular cryochamber project (a dir with `cryo.toml`):

```
~/my-cryo-workspace/
  chambers/
    chess-by-mail/     # cryo.toml + plan.md here
    mr-lazy/
    reports/
```

Start the UI from the workspace dir:

```bash
cd ~/my-cryo-workspace
cryo web           # installs a service that survives reboot
cryo web --foreground   # run in foreground (no service)
cryo web --stop    # stop and remove the service
```

## What the UI does

- **Sidebar** — every chamber, sorted by running → stopped → external. Shows status dot, name, unread-message badge.
- **Main pane** — full detail for the selected chamber: status, task, next wake, notes, message history, log tail, send widget.
- **Lifecycle buttons** — `start` / `stop` / `restart` for workspace chambers. External chambers show no lifecycle buttons.

## External chambers

Running daemons anywhere on the machine (registered via `cryo start` from any working directory) appear as **external** chambers if they aren't under the current workspace's `./chambers/`. They're monitor-only from the UI.

## Migrating from single-chamber mode

Earlier versions of `cryo web` ran inside a chamber and served that one chamber. To migrate:

```bash
mkdir -p ~/cryo-workspace/chambers
ln -s $(pwd) ~/cryo-workspace/chambers/my-chamber
cd ~/cryo-workspace && cryo web
```

Running `cryo web` from a chamber dir now prints a migration error.

## Security

The default bind is `127.0.0.1`. If you pass `--host 0.0.0.0`, cryo prints a warning because lifecycle actions are exposed over the network without authentication. Don't do that on a shared network. Token auth is tracked as future work.
