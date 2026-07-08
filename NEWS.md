# News

## Unreleased

Security and reliability hardening.

### Cryohub

- **Dashboard speaks Chinese.** The cryohub web UI ships a Simplified Chinese
  (zh-CN) translation alongside English. A language toggle in the top bar
  switches the whole UI; the choice persists and a Chinese-locale browser
  defaults to Chinese on first visit. Server-provided strings (status
  summaries, dates, message bodies) remain in their source language.

### Security

- **Hub hardened against cross-origin abuse.** The dashboard now validates the
  `Host` header (blocking DNS-rebinding) and enforces CSRF protection on
  lifecycle actions.
- **Secrets stay out of status.** Provider `env` values (API keys) are no
  longer echoed in `cryo status` or the hub's status views.
- **Log and message frontmatter are sanitized**, so untrusted inbox content
  cannot forge log lines or message headers.
- **`cryo init` writes a chamber `.gitignore`** covering `.cryo/`, keeping
  `.cryo/zuliprc` and other runtime state out of version control.
- **LICENSE added** to the repository.

### Reliability

- **Socket IPC has timeouts**, so a stuck agent or daemon can no longer hang a
  `cryo-agent` call indefinitely.
- **Sessions terminate by process group**, so a crashed or timed-out agent
  cannot leave orphaned child processes behind.
- **Inbox messages waiting at startup wake the agent** instead of sitting until
  the next scheduled wake.
- **Zulip sync uses HTTP timeouts**, so an unresponsive Zulip server no longer
  stalls the sync daemon.
- **Default session timeout applied** and `cryo-agent send` made more robust.
- Removed an unused dependency.

### Deprecated

- **GitHub Discussions sync (`cryo-gh`) is deprecated** and may be removed in a
  future release. In single-account setups the pull path drops your own
  comments, so inbound messages are unreliable — prefer Zulip sync
  (`cryo-zulip`).

### Removed

- **GitHub Discussions message sync (`cryo-gh`) removed.** The `cryo-gh` binary
  and its Discussion-based sync channel are gone. Use Zulip (`cryo-zulip`) for
  remote sync, or the local file-based inbox/outbox.

## v0.2.5 — 2026-05-19

- **Daily digests replace the report interval.** Periodic status reports are
  now daily digests derived from the session log; the old `report_time` /
  `report_interval` settings are gone.
- **Untrusted inbox wake sources are surfaced**, so you can tell when a wake
  came from an unverified sender.
- **`cryo restart` and `cryohub restart` no longer reinstall the OS service** —
  they restart the existing one in place.

## v0.2.4 — 2026-05-12

- **`watch_dirs` replaces `watch_inbox`.** Configure any list of directories to
  watch for reactive wake. The default is `["messages/inbox"]`; `[]` disables
  reactive wake entirely.
- **Daemon stand-in replies differentiate by hibernate state**, so a fallback
  message reads differently depending on how the session ended.

## v0.2.3 — 2026-05-04

- **Documentation restructured** into a Diátaxis mdbook (tutorial, how-to,
  reference, explanation), published to GitHub Pages.
- **Protocol template invariants restored.**

## v0.2.2 — 2026-05-03

- **Protocol prompt embedded in the binary**, so every session gets the
  canonical protocol without relying on an external file.
- **Cryohub chamber discovery expanded** and the dashboard UI improved.

## v0.2.1 — 2026-04-26

- **`cryo-agent dialog`** renders the cross-session conversation transcript and
  archives any pending inbox batch as a side effect.
- **Chamber scaffolding flow.** `cryo init` now seeds `README.md` and
  `NOTES.md` alongside `plan.md` and `cryo.toml`.

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
