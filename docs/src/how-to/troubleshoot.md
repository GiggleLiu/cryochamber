# Troubleshooting

For background on why these things happen, see [Concepts](../explanation/concepts.md). For commands referenced below, see [CLI reference](../reference/cli.md).

## Common errors

### `Error: cryo.toml not found`

You have not initialized the chamber.

**Fix.** Run `cryo init` in the chamber directory.

### `Error: plan.md not found`

`cryo start` requires a `plan.md` in the working directory.

**Fix.** Create `plan.md`, or run `cryo init` to generate a template.

### `Error: agent command 'opencode' not found on PATH`

The configured agent binary is not installed.

**Fix.** Either install it, or switch to a different agent:

```bash
cryo start --agent claude
# or edit cryo.toml: agent = "claude"
```

### `Error: daemon already running`

A daemon is already active for this chamber.

**Fix.** Check with `cryo status`, then stop the existing daemon with `cryo cancel` before starting a new one.

### `Error: connection refused` from `cryo-agent` commands

The daemon is not running. `cryo-agent` talks to the daemon over a Unix socket.

**Fix.** Start the daemon with `cryo start`.

### `cryo status` shows "stale PID"`

The daemon process died without cleaning up.

**Fix.** Run `cryo cancel` to clear the stale state, then `cryo start` again.

## Behavior questions

### What happens if my computer sleeps or reboots?

**Sleep.** The daemon process is suspended with the rest of the machine. When the machine resumes, the daemon notices the scheduled wake time has passed, runs the session immediately, and includes a `DELAYED WAKE` notice in the agent's prompt with the original scheduled time and how late the session is.

**Reboot.** The daemon is installed as an OS service, launchd on macOS and systemd on Linux, and restarts automatically after reboot. To use a plain background process instead, set `CRYO_NO_SERVICE=1` before running `cryo start`.

### How do I manually wake a sleeping daemon?

Run `cryo wake` to send a wake message to the daemon's inbox. You can include text:

```bash
cryo wake "Please check the latest PR"
```

- If inbox watching is enabled, the default, the daemon wakes immediately.
- If inbox watching is disabled, `cryo wake` sends `SIGUSR1` to force the daemon awake.
- If no daemon is running, the message is queued for the next `cryo start`.

`cryo send --wake` has the same effect.

### The agent keeps crashing and getting re-woken

Crashes cause the daemon to create fresh retry TODOs with backoff until a session succeeds or the operator stops the cycle. See [TODO lifecycle](../explanation/concepts.md#todo-lifecycle) for why retries happen.

**Diagnose.**

- Inspect the TODO list with `cryo-agent todo list`, or open `todo.json`.
- Read the agent's raw output in `cryo-agent.log`.

Common causes:

- The agent is hitting rate limits; set `max_session_duration` to throttle.
- A required dependency is missing in the chamber directory.
- The agent does not understand the `cryo-agent` protocol; check the session prompt in `cryo-agent.log`.

**Break the cycle.** Remove the retry TODO with `cryo-agent todo remove <id>`, or edit `todo.json`, fix the underlying issue, then add a fresh TODO.

## Windows

Cryochamber does not run on Windows. The daemon's IPC uses Unix domain sockets, wake signalling uses POSIX signals (SIGUSR1), and reboot persistence uses launchd (macOS) or systemd (Linux). Use WSL2 if you need Cryochamber on a Windows machine.
