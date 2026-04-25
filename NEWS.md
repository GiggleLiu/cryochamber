# News

## v0.2.0 — 2026-04-25

User-facing improvements over v0.1.2.

### New

- **Web dashboard for all your chambers.** A new `cryohub` binary serves a
  multi-chamber web UI over HTTP. Run it once at the workspace level and you
  can monitor, start/stop, and view notes for every chamber under that
  directory in your browser. Installs as a launchd/systemd service so it
  survives reboots.
- **Smarter wake scheduling.** Wake times are now derived from the agent's
  TODO list rather than a one-shot `--wake` argument. The agent writes a
  TODO with a time, and the daemon wakes for it — so the schedule you see
  in `cryo status` matches what the agent actually plans to do, and
  rescheduling is just editing a TODO.
- **`cryo status` shows next wake.** Status output now displays the upcoming
  TODO-derived wake time, so you can see at a glance when the agent will
  run next.
- **Alerts go through your sync channel.** Removed desktop pop-up
  notifications (`notify-rust`). All alerts now flow through the outbox, so
  they reach you over whichever channel you've configured (GitHub
  Discussions, Zulip, or local) — meaning you get them on your phone or
  email, not just on the machine running the daemon.

### Reliability

- **Daemon survives crashes more cleanly.** If the daemon crashes between
  scheduling a fallback alert and sending it, the alert is replayed on next
  startup with a `(replay after crash)` prefix instead of being silently
  lost.
- **Sync stops looping on bad config.** If your GitHub token is wrong or
  your Zulip stream doesn't exist, the sync daemon now halts cleanly with a
  visible reason, instead of either retrying forever in silence (Zulip) or
  dying opaquely (GitHub). Transient network errors still retry as before.
- **`make release` works on macOS.** Previously broke due to a BSD `sed`
  incompatibility.

### Migration notes

- `cryo-agent hibernate --wake <time>` no longer accepts `--wake`. Use
  `cryo-agent todo` to schedule the next wake.
- Desktop notifications are gone — configure a sync channel (GitHub or
  Zulip) if you want to be reached when away from the machine.
