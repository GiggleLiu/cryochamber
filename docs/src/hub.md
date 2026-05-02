# Cryohub

Cryohub is a global web dashboard for managing multiple cryochamber chambers from one browser tab. By default it serves the dashboard on `http://127.0.0.1:8765`.

![cryohub dashboard with the mr-lazy chamber selected](./images/cryohub-dashboard.png)

## How cryohub discovers chambers

Cryohub reads the user registry. It does not scan the directory where you start it. Chambers enter the registry when:

- `cryo start` runs inside a chamber.
- You create a chamber from the dashboard.

Clean daemon shutdown clears the PID but keeps the registry entry, so Cryohub shows stopped chambers too. Each refresh checks registered paths and prunes entries whose chamber directory or `cryo.toml` disappeared.

The registry lives under `$XDG_STATE_HOME/cryo/chambers/`, or `~/.cryo/chambers/` if `XDG_STATE_HOME` is unset.

## Chamber root and config

The dashboard's **New Chamber** button creates chambers under the configured chamber root. The default is:

```text
~/.cryo/chambers
```

Cryohub settings live in `$XDG_CONFIG_HOME/cryo/cryohub.toml`, or `~/.config/cryo/cryohub.toml` if `XDG_CONFIG_HOME` is unset:

```toml
host = "127.0.0.1"
port = 8765
chamber_root = "/Users/alice/.cryo/chambers"
```

For project-owned chamber collections, set `chamber_root` to a project path such as `/path/to/project/.cryo/chambers`.

## Start the hub

1. Start the hub from any directory. Choose one of:

   ```bash
   cryohub start               # install as a service (survives reboot)
   cryohub start --foreground  # run in the current terminal (no service)
   ```

2. Open the URL printed by `cryohub` in your browser.

`--host` and `--port` override the config file and update the saved hub config.

## Stop the hub

1. From any directory, run:

   ```bash
   cryohub stop
   ```

   This uninstalls the global hub service.

2. (Optional) Confirm the service is gone:

   ```bash
   cryohub status
   ```

## Check status

`cryohub status` prints the global service status, URL, chamber root, config path, and log path if a log exists. It also lists legacy cwd-scoped services from older Cryohub versions so you can remove them.

## Use the dashboard

The web UI has two main areas:

- **Sidebar** — every registered chamber, sorted by running state and name. Each row shows a status dot, the chamber name, and an unread-message badge. Chambers under the configured chamber root show only their folder name; chambers elsewhere show a compact parent-path hint.
- **Main pane** — full detail for the selected chamber: status, current task, next wake time, notes, message history, log tail, and a send widget.

Lifecycle buttons are available for registered chambers. Archive is disabled in the global hub because there is no directory-scoped workspace to archive from.

## Security

The default bind address is `127.0.0.1`, so the dashboard is only reachable from the local machine.

> **Warning**: Passing `--host 0.0.0.0` exposes lifecycle actions over the network without authentication. Cryohub prints a warning when you do this. Don't use `0.0.0.0` on a shared or untrusted network. Token authentication is tracked as future work.
