# cryo-gh — GitHub Discussion Sync

`cryo-gh` bridges a cryochamber project with a GitHub Discussion, enabling remote monitoring and two-way messaging. Discussion comments become inbox messages for the agent; outbox messages from the agent become Discussion comments.

For general project setup and CLI reference, see the [README](../README.md).

## Prerequisites

- [GitHub CLI](https://cli.github.com) (`gh`) installed and authenticated (`gh auth login`)
- A GitHub repository where you have write access
- An initialized cryochamber project (`cryo init`)

## Commands

```bash
cryo-gh init --repo owner/repo   # Create a Discussion and write gh-sync.json
cryo-gh sync [--interval 30]     # Start background sync daemon
cryo-gh unsync                   # Stop the sync daemon
cryo-gh pull                     # One-shot: pull new comments → inbox
cryo-gh push                     # One-shot: push latest session log → Discussion
cryo-gh status                   # Show sync configuration
```

## How Sync Works

`cryo-gh sync` spawns a background daemon (just like `cryo start` does). It does two things in a loop:

**Discussion → Inbox** (pull direction): Polls the GitHub Discussion for new comments every `--interval` seconds (default: 30). New comments are written to `messages/inbox/` where the daemon picks them up on the next session.

**Outbox → Discussion** (push direction): Watches `messages/outbox/` for new files. When the agent sends a message (via `cryo-agent send`), the sync daemon posts it as a Discussion comment and archives the file to `messages/outbox/archive/`.

```text
GitHub Discussion                  Local filesystem
─────────────────                  ─────────────────
New comment        ──(pull)──→     messages/inbox/       → agent reads on wake
                   ←─(push)──     messages/outbox/      ← agent writes via cryo-agent send
```

The sync is managed as a system service (launchd on macOS, systemd on Linux) that **survives reboots**. Logs go to `cryo-gh-sync.log`.

## Recommended Workflow

### 1. Initialize the project

```bash
cryo init --agent claude
# edit plan.md with your task
```

### 2. Link to GitHub

```bash
cryo-gh init --repo owner/repo
```

This creates a Discussion in the repository (enabling Discussions automatically if needed) and writes `gh-sync.json` with the Discussion number and node ID.

### 3. Start the daemon and sync

```bash
cryo start
cryo-gh sync
```

Both run as background daemons. Monitor with `cryo watch`.

### 4. Send messages from GitHub

Post a comment on the Discussion from the GitHub web UI or mobile app. The sync daemon picks it up within 30 seconds and writes it to `messages/inbox/`. The daemon wakes the agent on the next session (or immediately if `watch_inbox = true`).

### 5. Read agent replies on GitHub

When the agent calls `cryo-agent send "message"`, the outbox file is detected immediately by the sync watcher and posted as a Discussion comment.

### 6. Stop

```bash
cryo-gh unsync   # stop sync daemon
cryo cancel      # stop cryo daemon
```

## One-Shot Usage

For manual or scripted use without the sync daemon:

```bash
cryo-gh pull    # fetch new Discussion comments into inbox
cryo-gh push    # post the latest session log to the Discussion
```

## Rate Limits

The sync daemon uses the `gh` CLI which makes authenticated GitHub API requests. At the default 30-second interval, this is ~120 requests/hour — well within GitHub's 5,000 requests/hour limit for authenticated users.

## Files

| File | Purpose |
|------|---------|
| `gh-sync.json` | Sync state: repo, Discussion number/ID, cursor, sync daemon PID |
| `cryo-gh-sync.log` | Sync daemon log output |
| `messages/inbox/` | Incoming messages (from Discussion comments) |
| `messages/outbox/` | Outgoing messages (posted to Discussion) |
| `messages/outbox/archive/` | Posted outbox messages (archived after sync) |
