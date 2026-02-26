# FAQ

## What happens if my computer sleeps or reboots?

**Sleep:** The daemon process is suspended along with everything else. When your machine wakes up, the daemon resumes and detects that the scheduled wake time has passed. It runs the session immediately and includes a "DELAYED WAKE" notice in the agent's prompt with the original scheduled time and how late the session is.

**Reboot:** The daemon is installed as an OS service (launchd on macOS, systemd on Linux) and restarts automatically after reboot. Set `CRYO_NO_SERVICE=1` before `cryo start` to disable this and use a plain background process instead.

## How do I manually wake a sleeping daemon?

Use `cryo wake` to send a message to the daemon's inbox. You can include a message: `cryo wake "Please check the latest PR"`. If inbox watching is enabled (the default), the daemon wakes immediately. You can also use `cryo send --wake` for the same effect. If inbox watching is disabled, `cryo wake` sends a SIGUSR1 signal to force the daemon awake. If no daemon is running, the message is queued for the next `cryo start`.

## Troubleshooting

### `Error: cryo.toml not found`

You haven't initialized the project. Run `cryo init` first.

### `Error: plan.md not found`

`cryo start` requires a `plan.md` in the working directory. Create one or run `cryo init` to generate a template.

### `Error: agent command 'opencode' not found on PATH`

The configured agent isn't installed. Either install it or change the agent:

```bash
cryo start --agent claude       # use a different agent
# or edit cryo.toml: agent = "claude"
```

### `Error: daemon already running`

A daemon is already active for this project. Check with `cryo status`, or stop it with `cryo cancel` before starting a new one.

### `Error: connection refused` (from `cryo-agent` commands)

The daemon isn't running. The `cryo-agent` CLI needs a running daemon to communicate with via the Unix socket. Start the daemon with `cryo start`.

### `cryo status` shows "stale PID"

The daemon process died without cleaning up. Run `cryo cancel` to clear the stale state, then `cryo start` again.

### Agent keeps crashing (retries exhausted)

Check `cryo-agent.log` for the agent's raw output. Common causes:
- Agent hitting rate limits (add `max_session_duration` to throttle)
- Missing dependencies in the project
- Agent doesn't understand the `cryo-agent` protocol (check the generated AGENTS.md/CLAUDE.md)

### `cryo-gh`: `gh: command not found`

Install the [GitHub CLI](https://cli.github.com) and authenticate: `gh auth login`.

### `cryo-gh`: `no gh-sync.json found`

Run `cryo-gh init --repo owner/repo` to create a Discussion and initialize sync state.
