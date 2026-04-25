# GitHub Discussion sync

`cryo-gh` bridges a chamber with a GitHub Discussion, giving you remote monitoring and two-way messaging from anywhere you can use GitHub. Comments on the Discussion become inbox messages for the agent, and outbox messages from the agent appear as new comments on the Discussion.

## Prerequisites

Before you begin, make sure you have:

- The [GitHub CLI](https://cli.github.com) (`gh`) installed and authenticated. Run `gh auth login` if you haven't already.
- A GitHub repository where you have write access.
- An initialized cryochamber project. See [Getting Started](./getting-started.md) if you don't have one yet.

## Set up sync

1. Create a Discussion and write the sync state:

   ```bash
   cryo-gh init --repo owner/repo
   ```

   This creates a Discussion in the repository (enabling Discussions automatically if needed) and writes `gh-sync.json` with the Discussion number and node ID.

2. Start the cryo daemon:

   ```bash
   cryo start
   ```

3. Start the sync daemon:

   ```bash
   cryo-gh sync
   ```

   By default this polls GitHub every 5 seconds. Override the interval with `--interval 30`.

4. Verify both daemons are running:

   ```bash
   cryo status
   cryo-gh status
   ```

Both daemons run as system services (launchd on macOS, systemd on Linux) and survive reboots. Sync logs go to `cryo-gh-sync.log`.

## Send a message from GitHub

1. Open the Discussion in the GitHub web UI or mobile app.
2. Post a comment.
3. Within the poll interval (default 5s), the sync daemon writes the comment to `messages/inbox/`.
4. The cryo daemon wakes the agent on the next session, or immediately if `watch_inbox = true` in `cryo.toml`.

## Read agent replies on GitHub

When the agent runs `cryo-agent send "message"`, the sync daemon detects the new outbox file and posts it as a Discussion comment within seconds.

## Stop sync

1. Stop the sync daemon:

   ```bash
   cryo-gh unsync
   ```

2. (Optional) Stop the cryo daemon as well:

   ```bash
   cryo cancel
   ```

## How sync works

`cryo-gh sync` runs a background loop that does two things:

| Direction | What happens |
|-----------|--------------|
| **Discussion → inbox** (pull) | Polls the Discussion for new comments every `--interval` seconds. New comments are written to `messages/inbox/`. |
| **Outbox → Discussion** (push) | Watches `messages/outbox/` for new files. New files are posted as comments and archived to `messages/outbox/archive/`. |

```text
GitHub Discussion                  Local filesystem
─────────────────                  ─────────────────
New comment        ──(pull)──→     messages/inbox/       → agent reads on wake
                   ←─(push)──      messages/outbox/      ← agent writes via cryo-agent send
```

## One-shot pull and push

For manual or scripted use without the sync daemon:

```bash
cryo-gh pull    # fetch new Discussion comments into messages/inbox/
cryo-gh push    # post the latest session log as a Discussion comment
```

## Command reference

| Command | What it does |
|---------|--------------|
| `cryo-gh init --repo owner/repo` | Create a Discussion and write `gh-sync.json`. |
| `cryo-gh sync [--interval N]` | Start the background sync daemon. Default interval comes from `cryo.toml` or falls back to 5 seconds. |
| `cryo-gh unsync` | Stop the sync daemon. |
| `cryo-gh pull` | One-shot pull. |
| `cryo-gh push` | One-shot push. |
| `cryo-gh status` | Show sync configuration. |

## Rate limits

The sync daemon authenticates through `gh`, which counts against your authenticated GitHub API quota of 5,000 requests/hour. At the default 5-second interval, sync makes ~720 requests/hour — well within the limit. If you run many chambers, raise `--interval` or use a personal access token with a higher quota.

## Files

| File | Purpose |
|------|---------|
| `gh-sync.json` | Sync state: repo, Discussion number/ID, last read position. |
| `cryo-gh-sync.log` | Sync daemon log output. |
| `messages/inbox/` | Incoming messages (from Discussion comments). |
| `messages/outbox/` | Outgoing messages (posted to the Discussion). |
| `messages/outbox/archive/` | Archived outbox messages after they are posted. |
