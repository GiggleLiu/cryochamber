# Cryohub

Cryohub is a web dashboard for managing multiple cryochamber chambers from one browser tab. It starts from a workspace directory and, by default, also shows known chambers that were started elsewhere under the same user account. By default it serves the dashboard on `http://127.0.0.1:8765`.

![cryohub dashboard with the mr-lazy chamber selected](./images/cryohub-dashboard.png)

## How cryohub discovers chambers

Cryohub always operates from the current working directory. It scans every immediate subdirectory and treats any subdirectory containing a `cryo.toml` as a chamber. It also reads the user-level known-chambers registry so stopped or running chambers from other folders can appear in the same dashboard.

A typical workspace layout:

```text
~/my-chambers/
  chess-by-mail/     # cryo.toml + plan.md here
  mr-lazy/
  reports/
```

You start the hub from the workspace root (`~/my-chambers`), not from a chamber directory.

> **Note**: Cryohub refuses to start in a directory that itself contains a `cryo.toml`. Move up one level first.

`cryo start` records that chamber in the known-chambers registry. On hub startup and refresh, Cryohub prunes registry entries whose directory is gone or no longer contains `cryo.toml`. To keep the hub strictly workspace-scoped, start it with `--local-only`.

## Start the hub

1. Change into the directory that holds your chambers as subdirectories:

   ```bash
   cd ~/my-chambers
   ```

2. Start the hub. Choose one of:

   ```bash
   cryohub start                       # install as a service (survives reboot)
   cryohub start --foreground          # run in the current terminal (no service)
   cryohub start --local-only          # show only this workspace
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

- **Sidebar** — every discovered chamber, sorted by running state with completed chambers folded into history. Workspace chambers show their folder name; known chambers from elsewhere also show a muted path line.
- **Main pane** — full detail for the selected chamber: status, current task, next wake time, notes, message history, log tail, and a send widget.

Path-safe lifecycle actions such as **Start**, **Stop**, **Wake**, **Send**, and **Reset** are available for any valid known chamber. **Archive** and **New Chamber** stay tied to the current workspace.

## Known chambers elsewhere

If a chamber was started from another directory, Cryohub can still show it through the user-level known-chambers registry. These rows are not placed in a separate section or marked with a badge; the muted path line is the distinction.

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
