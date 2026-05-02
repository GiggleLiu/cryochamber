# FAQ and troubleshooting

## What happens if my computer sleeps or reboots?

**Sleep.** The daemon process is suspended along with everything else. When the machine resumes, the daemon notices the scheduled wake time has passed, runs the session immediately, and includes a `DELAYED WAKE` notice in the agent's prompt with the original scheduled time and how late the session is.

**Reboot.** The daemon is installed as an OS service (launchd on macOS, systemd on Linux) and restarts automatically after reboot. To use a plain background process instead, set `CRYO_NO_SERVICE=1` before running `cryo start`.

## How do I manually wake a sleeping daemon?

Run `cryo wake` to send a wake message to the daemon's inbox. You can include text:

```bash
cryo wake "Please check the latest PR"
```

- If `watch_inbox` is enabled (the default), the daemon wakes immediately.
- If `watch_inbox` is disabled, `cryo wake` sends a `SIGUSR1` signal to force the daemon awake.
- If no daemon is running, the message is queued for the next `cryo start`.

`cryo send --wake` has the same effect.

## Troubleshooting

### `Error: cryo.toml not found`

You haven't initialized the chamber.

**Fix.** Run `cryo init` in the chamber directory.

### `Error: plan.md not found`

`cryo start` requires a `plan.md` in the working directory.

**Fix.** Create `plan.md`, or run `cryo init` to generate a template.

### `Error: agent command 'opencode' not found on PATH`

The configured agent binary isn't installed.

**Fix.** Either install it, or switch to a different agent:

```bash
cryo start --agent claude       # one-shot override
# or edit cryo.toml: agent = "claude"
```

### `Error: daemon already running`

A daemon is already active for this chamber.

**Fix.** Check with `cryo status`, then stop the existing daemon with `cryo cancel` before starting a new one.

### `Error: connection refused` from `cryo-agent` commands

The daemon isn't running. `cryo-agent` talks to the daemon over a Unix socket.

**Fix.** Start the daemon with `cryo start`.

### `cryo status` shows "stale PID"

The daemon process died without cleaning up.

**Fix.** Run `cryo cancel` to clear the stale state, then `cryo start` again.

### The agent keeps crashing and getting re-woken

A crashed session re-injects the TODO that triggered it with an `(attempt k)` suffix and a `2^k`-minute delay (capped at 1 day), so the chamber keeps retrying at growing intervals.

**Diagnose.**

- Inspect the TODO list: `cryo-agent todo list` (from inside the chamber) or open `todo.json`.
- Read the agent's raw output in `cryo-agent.log`.

Common causes:

- The agent is hitting rate limits — set `max_session_duration` to throttle.
- A required dependency is missing in the chamber directory.
- The agent doesn't understand the `cryo-agent` protocol — check the session prompt in `cryo-agent.log`; Cryochamber embeds the protocol in every prompt.

**Break the cycle.** Remove the `(attempt k)` TODO with `cryo-agent todo remove <id>` (or edit `todo.json`), fix the underlying issue, then add a fresh TODO.

### `cryo-gh`: `gh: command not found`

The GitHub CLI is not installed.

**Fix.** Install the [GitHub CLI](https://cli.github.com), then authenticate with `gh auth login`.

### `cryo-gh`: `no gh-sync.json found`

The chamber has not been linked to a Discussion.

**Fix.** Run `cryo-gh init --repo owner/repo` to create a Discussion and initialize sync state.
