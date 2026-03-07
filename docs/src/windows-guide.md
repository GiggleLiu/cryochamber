# Windows User Guide

Complete guide for using cryochamber on Windows.

## Installation

### Prerequisites

- Windows 10 or later
- Rust toolchain (install from [rustup.rs](https://rustup.rs))
- Administrator privileges (for Windows Service installation)

### Install from Source

```bash
git clone https://github.com/GiggleLiu/cryochamber.git
cd cryochamber
cargo install --path .
```

Verify installation:

```bash
cryo --version
```

## Quick Start

### 1. Initialize Project

```bash
mkdir my-project
cd my-project
cryo init
```

This creates:
- `cryo.toml` - Configuration
- `AGENTS.md` - Agent protocol
- `plan.md` - Task plan template
- `README.md` - Project guide
- `messages/inbox/` - Incoming messages
- `messages/outbox/` - Outgoing messages

### 2. Edit Your Plan

Open `plan.md` and describe your task:

```markdown
# Task: Build a web scraper

## Goal
Create a Python script that scrapes product prices from example.com

## Steps
1. Install requests and beautifulsoup4
2. Write scraper.py
3. Test with sample URL
4. Add error handling
```

### 3. Start the Daemon

```bash
cryo start
```

**Important**: This requires Administrator privileges to install a Windows Service.

The service:
- Runs in the background
- Survives reboots
- Wakes on schedule or when messages arrive
- Logs to `cryo.log` and `cryo-agent.log`

### 4. Check Status

```bash
cryo status
```

Output:
```
Daemon: running
Session: 1
Next wake: idle (no pending TODOs)
PID: 12345
Agent: opencode
```

### 5. Send Messages

```bash
cryo send "Please add unit tests" --wake
```

The `--wake` flag immediately wakes the agent to process your message.

### 6. Monitor Progress

Watch logs in real-time:

```bash
cryo watch
```

Or view the full log:

```bash
cryo log
```

### 7. Stop the Daemon

```bash
cryo cancel
```

This stops and removes the Windows Service.

## Windows Service Details

### Service Installation

When you run `cryo start`, it:
1. Creates a Windows Service named `com.cryo.daemon.<hash>`
2. Configures it to start automatically
3. Starts the service immediately

The service runs as LocalSystem and executes:
```
cryo.exe daemon --dir "C:\path\to\your\project"
```

### Service Management

Check if service is running:
```bash
cryo status
```

Restart the service:
```bash
cryo restart
```

List all running cryochamber daemons:
```bash
cryo ps
```

Kill all daemons:
```bash
cryo ps --kill-all
```

### Running Without Service (No Admin)

If you don't have Administrator privileges:

```bash
set CRYO_NO_SERVICE=1
cryo start
```

This runs the daemon as a regular background process (won't survive reboots).

## Configuration

### cryo.toml

```toml
agent = "opencode run"
max_retries = 3
max_session_duration = 3600
watch_inbox = true
```

- `agent`: Command to run (default: `opencode run`)
- `max_retries`: Retry attempts on failure
- `max_session_duration`: Timeout in seconds
- `watch_inbox`: Auto-wake on new messages

### Override at Runtime

```bash
cryo start --agent "claude-code" --max-retries 5
```

CLI flags override `cryo.toml` for that session.

## Messaging

### Send Messages

```bash
# Simple message
cryo send "Add error handling to the scraper"

# With custom sender
cryo send "Review the code" --from "reviewer"

# With subject
cryo send "Bug report" --subject "Scraper crashes on timeout"

# Wake immediately
cryo send "Urgent fix needed" --wake
```

Messages are saved to `messages/inbox/` as Markdown files.

### Receive Messages

```bash
cryo receive
```

Shows messages from `messages/outbox/` (agent replies).

## Troubleshooting

### "Access Denied" Error

**Problem**: `cryo start` fails with "Access Denied"

**Solution**: Run Command Prompt or PowerShell as Administrator:
1. Right-click Command Prompt
2. Select "Run as administrator"
3. Navigate to your project
4. Run `cryo start`

**Alternative**: Use no-service mode:
```bash
set CRYO_NO_SERVICE=1
cryo start
```

### Service Won't Start

**Check logs**:
```bash
type cryo.log
```

**Common causes**:
- Agent command not found (check `cryo.toml`)
- Invalid `plan.md` syntax
- Port already in use

**Fix**:
```bash
cryo cancel
# Fix the issue
cryo start
```

### Daemon Shows "Stopped" But Service is Running

This was a bug in versions before 0.1.3. Update to the latest version:

```bash
cargo install --path . --force
```

### Agent Not Responding

**Check if agent is running**:
```bash
cryo status
```

**View agent output**:
```bash
type cryo-agent.log
```

**Restart**:
```bash
cryo restart
```

### Clean Start

Remove all runtime files:

```bash
cryo clean --force
```

This deletes:
- `cryo.log`
- `cryo-agent.log`
- `timer.json`
- `todo.json`
- Messages in `inbox/archive/`

## Advanced Usage

### Custom Agent

Edit `cryo.toml`:

```toml
agent = "python my_agent.py"
```

Your agent must:
1. Read from stdin or files
2. Call `cryo-agent hibernate` when done
3. Use `cryo-agent note` for logging

### Scheduled Wake

```bash
cryo-agent todo add "Daily report" --daily 09:00
```

The daemon will wake at 9 AM daily.

### Multiple Projects

Each project directory has its own daemon:

```bash
cd project1
cryo start

cd ../project2
cryo start

cryo ps  # Shows both daemons
```

### Inbox Watching

When `watch_inbox = true` in `cryo.toml`, the daemon automatically wakes when new files appear in `messages/inbox/`.

Disable for manual control:

```toml
watch_inbox = false
```

Then wake manually:

```bash
cryo wake
```

## File Locations

### Project Files

```
my-project/
├── cryo.toml              # Configuration
├── AGENTS.md              # Agent protocol
├── plan.md                # Task plan
├── cryo.log               # Daemon event log
├── cryo-agent.log         # Agent stdout/stderr
├── timer.json             # Runtime state (PID, session)
├── todo.json              # TODO list
└── messages/
    ├── inbox/             # Incoming messages
    │   └── archive/       # Processed messages
    └── outbox/            # Outgoing messages
```

### System Files

- Service registry: Windows Service Control Manager
- No global config files (all per-project)

## Best Practices

1. **Always edit plan.md before starting** - The agent needs clear instructions

2. **Use version control** - Commit `cryo.toml`, `plan.md`, and `AGENTS.md`

3. **Don't commit runtime files** - Add to `.gitignore`:
   ```
   cryo.log
   cryo-agent.log
   timer.json
   todo.json
   messages/
   ```

4. **Monitor logs** - Use `cryo watch` during development

5. **Clean up** - Run `cryo cancel` when done to remove the service

6. **Test without service first** - Use `CRYO_NO_SERVICE=1` for testing

## Next Steps

- Read [Agent Protocol](./agent-protocol.md) to understand how agents work
- See [Examples](./examples.md) for real-world use cases
- Check [GitHub Sync](./github-sync.md) for team collaboration
