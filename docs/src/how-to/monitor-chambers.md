# Monitor and message a chamber

There are three ways to monitor a chamber and exchange messages with the agent. **Cryohub is the primary monitor**; **Zulip** is the recommended remote and mobile channel. GitHub Discussions sync (`cryo-gh`) still works but is **deprecated** — see the warning below.

You can run them together. Cryohub is local-only by default; the sync channels reach the same chamber from the outside.

## Cryohub (primary, recommended)

Cryohub is a global web dashboard that manages every chamber registered on this machine. It can be started, restarted, and stopped from any directory.

![cryohub dashboard with the mr-lazy chamber selected](../images/cryohub-dashboard.png)

### Start the hub

```bash
cryohub start
cryohub start --foreground
```

`cryohub` prints the local dashboard URL. Open it in a browser.

### Use the dashboard

- **Sidebar** - every registered chamber, sorted by running state and name. Each row shows a status dot and an unread-message badge.
- **Main pane** - full detail for the selected chamber: status, current task, next wake time, notes, message history, log tail, and a send widget. Lifecycle buttons, Start, Stop, Restart, and Wake, sit next to the status.
- **New Chamber** modal - scaffolds a chamber under the configured chamber root, `~/.cryo/chambers` by default.

### Manage the hub service

```bash
cryohub restart
cryohub stop
cryohub status
```

Use `cryohub restart` to restart the installed dashboard service without reinstalling it. Use `cryohub status` to confirm the service state and see the configured URL, chamber root, config path, and log path.

### Security

The default bind address is `127.0.0.1`, so the dashboard is only reachable from the local machine.

> **Warning**: Passing `--host 0.0.0.0` exposes lifecycle actions over the network without authentication. Cryohub prints a warning when you do this. Do not use `0.0.0.0` on a shared or untrusted network. Token authentication is future work.

For chamber discovery internals, see [Concepts](../explanation/concepts.md). For full configuration fields, see [`cryohub.toml`](../reference/configuration.md#cryohubtoml).

## GitHub Discussions (remote, deprecated)

> **Deprecated.** GitHub Discussions sync is deprecated and not recommended for new chambers; it may be removed in a future release. In the documented single-account setup (you authenticate `gh` as yourself and comment from the same account), the pull path filters out your own comments, so your messages never reach the agent's inbox. Use [Zulip](#zulip-remote) instead. The reference below is kept for existing chambers.

`cryo-gh` bridges a chamber with a GitHub Discussion. Comments on the Discussion become inbox messages for the agent; outbox messages from the agent appear as new comments on the Discussion.

### Prerequisites

- The [GitHub CLI](https://cli.github.com), `gh`, installed and authenticated with `gh auth login`.
- A GitHub repository where you have write access.
- An initialized cryochamber project. See [Tutorial](../tutorial.md) if you do not have one.

### Set up

```bash
cryo-gh init --repo owner/repo
cryo start
cryo-gh sync
cryo-gh status
```

Both daemons run as OS services and survive reboots. Logs go to `cryo-gh-sync.log`. Use `cryo-gh sync --interval N` to override the default 5-second poll interval.

### Use it

Post a comment on the Discussion and it appears in `messages/inbox/` within the poll interval. When the agent sends an outbox message, `cryo-gh` posts it back to the Discussion within seconds.

### Stop

```bash
cryo-gh unsync
```

### Rate limits

`gh` counts against the authenticated GitHub API quota of 5,000 requests per hour. At the default 5-second interval, sync makes about 720 requests per hour. If you run many chambers, raise `--interval`.

For the full command list, see [CLI reference](../reference/cli.md#github-sync-cryo-gh).

## Zulip (remote)

`cryo-zulip` bridges a chamber with a Zulip stream and topic.

### Prerequisites

- A Zulip server with a bot account.
- A `zuliprc` file with bot credentials: an INI file whose `[api]` section contains `email`, `key`, and `site`.
- A Zulip stream the bot can read and post to.
- An initialized cryochamber project.

### Set up

```bash
cryo-zulip init --config ~/.zuliprc --stream my-stream
cryo start
cryo-zulip sync
cryo-zulip status
```

Optional flags for `init`: `--topic <name>`, default `cryochamber`, and `--history` to import existing stream messages on the first pull.

> **Warning**: Do not commit, push, or sync `.cryo/zuliprc` - it holds your bot's API key. The file is gitignored by default; never include it in messages or sync payloads.

### Use it

Post a message in the configured topic and it appears in `messages/inbox/` within the poll interval. The bot's own messages are filtered to avoid echo loops. Outbox messages from the agent appear in the topic within seconds.

### Stop

```bash
cryo-zulip unsync
```

For the full command list, see [CLI reference](../reference/cli.md#zulip-sync-cryo-zulip).

## Run multiple monitors together

Cryohub, `cryo-gh sync`, and `cryo-zulip sync` are independent daemons. They all read and write `messages/inbox/` and `messages/outbox/` for the same chamber, so:

- A message posted on GitHub appears in Cryohub's history after the next poll and can wake the agent.
- A message sent from Cryohub's send widget gets pushed to GitHub or Zulip by whichever sync daemons are running.

There is no extra configuration to combine them. Start the monitors you want.

For the underlying inbox/outbox bridge model, see [Concepts](../explanation/concepts.md#how-sync-channels-bridge-inboxoutbox).
