# Monitor and message a chamber

There are two ways to monitor a chamber and exchange messages with the agent. **Cryohub is the primary monitor**; **Zulip** is the recommended remote and mobile channel.

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

- **Sidebar** - every registered chamber, sorted by running state and name. Each row shows a status dot and an unread-message badge. Completed and archived chambers fold into collapsible **Completed** and **Archived** sections at the bottom.
- **Main pane** - full detail for the selected chamber: status, current task, next wake time, notes, message history, log tail, and a send widget. Lifecycle buttons, Start, Stop, Restart, and Wake, sit next to the status.
- **Archive** - a stopped chamber shows an **Archive** button that folds it out of the active list into the **Archived** section without moving any files. Archiving is reversible: an archived chamber offers only **Unarchive**, which returns it to the active list (still stopped) so you can Launch it again. A running chamber must be stopped first.
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

### Images and file attachments

Files uploaded to the topic (photos, screenshots, documents) are downloaded on pull into `messages/attachments/`, and the upload links in the inbox message are rewritten to those local paths — so an agent with vision can read an uploaded image directly. Details:

- Downloads are authenticated with the bot's API key and capped at 25 MB per file.
- If a download fails, the pull still succeeds: the original `/user_uploads/...` link is kept and the error is logged to `cryo-zulip-sync.log`.
- Attachment files are kept forever, like the message archives. Delete old files manually if disk space matters.

Sending images works the same way in reverse. If an agent reply links to a file inside the chamber, `cryo-zulip` uploads it on push and rewrites the link to the resulting `/user_uploads/` path, so it renders inline:

```bash
cryo-agent send "Here is the sketch: [qubit.png](messages/attachments/qubit.png)"
```

Details:

- Use ordinary link syntax, `[text](path)`. The sync rewrites the destination to an **absolute** URL and strips any leading `!`, which is what makes the preview appear: Zulip's inline-preview pass only matches absolute upload URLs, and it renders CommonMark image syntax (`![text](path)`) as literal text before server 12.0 (feature level 437).
- Only regular files inside the chamber are uploaded. Paths escaping the chamber and anything under `.cryo/` are refused, because that directory holds the bot's API key.
- Each distinct file uploads once per message. A failed upload leaves the link untouched and still posts the message, logging the reason to `cryo-zulip-sync.log`.

### Stop

```bash
cryo-zulip unsync
```

For the full command list, see [CLI reference](../reference/cli.md#zulip-sync-cryo-zulip).

## Run multiple monitors together

Cryohub and `cryo-zulip sync` are independent daemons. They both read and write `messages/inbox/` and `messages/outbox/` for the same chamber, so:

- A message posted on Zulip appears in Cryohub's history after the next poll and can wake the agent.
- A message sent from Cryohub's send widget gets pushed to Zulip by the sync daemon if it is running.

There is no extra configuration to combine them. Start the monitors you want.

For the underlying inbox/outbox bridge model, see [Concepts](../explanation/concepts.md#how-sync-channels-bridge-inboxoutbox).
