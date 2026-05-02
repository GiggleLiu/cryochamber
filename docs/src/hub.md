# Cryohub

Cryohub is a directory-scoped web dashboard for managing multiple cryochamber chambers from one browser tab. By default it serves the dashboard on `http://127.0.0.1:8765`.

![cryohub dashboard with the mr-lazy chamber selected](./images/cryohub-dashboard.png)

## How cryohub discovers chambers

Cryohub always operates on the current working directory. It scans every immediate subdirectory and treats any subdirectory containing a `cryo.toml` as a chamber. By default it also merges chambers remembered by the user registry, so chambers started elsewhere on the same machine remain visible after they stop.

A typical workspace layout:

```text
~/my-chambers/
  chess-by-mail/     # cryo.toml + plan.md here
  mr-lazy/
  reports/
```

You start the hub from the workspace root (`~/my-chambers`), not from a chamber directory.

> **Note**: Cryohub refuses to start in a directory that itself contains a `cryo.toml`. Move up one level first.

## Start the hub

1. Change into the directory that holds your chambers as subdirectories:

   ```bash
   cd ~/my-chambers
   ```

2. Start the hub. Choose one of:

   ```bash
   cryohub start               # install as a service (survives reboot)
   cryohub start --foreground  # run in the current terminal (no service)
   cryohub start --local-only  # ignore the user registry
   ```

3. Open the URL printed by `cryohub` in your browser.

## Stop the hub

1. From the same directory you started it in, run:

   ```bash
   cryohub stop
   ```

   This uninstalls the service for that directory.

2. (Optional) Confirm the service is gone:

   ```bash
   cryohub status
   ```

## Check status across the machine

`cryohub status` prints two sections:

- The status of the hub for the current directory.
- A list of every other `cryohub` service installed elsewhere on the machine.

This makes it easy to find a hub you started from a different working directory.

## Use the dashboard

The web UI has two main areas:

- **Sidebar** — every discovered chamber, sorted by running state and name. Each row shows a status dot, the chamber name, and an unread-message badge. Chambers outside the current workspace show a compact parent-path hint; local workspace chambers show only their folder name.
- **Main pane** — full detail for the selected chamber: status, current task, next wake time, notes, message history, log tail, and a send widget.

Lifecycle buttons are available for discovered chambers. Archive is workspace-local, so it is only offered for chambers under the hub's working directory.

## Registry chambers

When `cryo start` runs, the daemon records the chamber in the user registry under `$XDG_STATE_HOME/cryo/chambers/` (or `~/.cryo/chambers/` if `XDG_STATE_HOME` is unset). Clean daemon shutdown clears the PID but keeps the entry, so Cryohub can show stopped chambers too. Each hub refresh checks registered paths and prunes entries whose chamber directory or `cryo.toml` disappeared.

## Run the hub for a single chamber

If you only have one chamber and still want the hub UI, create a parent directory and symlink your chamber into it:

1. Create a parent directory:

   ```bash
   mkdir -p ~/cryo-chambers
   ```

2. Symlink your chamber into it:

   ```bash
   ln -s "$(pwd)" ~/cryo-chambers/my-chamber
   ```

3. Start the hub from the parent directory:

   ```bash
   cd ~/cryo-chambers && cryohub start
   ```

## Security

The default bind address is `127.0.0.1`, so the dashboard is only reachable from the local machine.

> **Warning**: Passing `--host 0.0.0.0` exposes lifecycle actions over the network without authentication. Cryohub prints a warning when you do this. Don't use `0.0.0.0` on a shared or untrusted network. Token authentication is tracked as future work.
