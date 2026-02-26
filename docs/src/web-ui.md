# Web UI

`cryo web` starts a local web server with a chat interface for sending messages to your agent and monitoring its activity.

## Usage

```bash
cryo web                         # default: http://127.0.0.1:3945
cryo web --port 8080             # custom port
cryo web --host 0.0.0.0          # listen on all interfaces
```

Or configure in `cryo.toml`:

```toml
web_host = "127.0.0.1"
web_port = 3945
```

## Features

- **Chat interface** — Send messages to the agent's inbox and see outbox replies
- **Status bar** — Shows daemon status (running/stopped), session number, and agent name
- **Wake button** — Force the daemon to wake immediately (sends SIGUSR1)
- **Live log** — Toggle the log panel to see `cryo.log` events in real-time
- **Real-time updates** — Server-Sent Events (SSE) stream new messages, status changes, and log lines as they happen
- **Polling fallback** — Periodic polling ensures messages from the daemon are never missed

## API Endpoints

The web server exposes a JSON API:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/` | GET | Chat UI (HTML) |
| `/api/status` | GET | Daemon status (running, session, agent, log tail) |
| `/api/messages` | GET | All messages (inbox + archived inbox + outbox), sorted by time |
| `/api/send` | POST | Send a message to inbox (`{ "body": "...", "from": "...", "subject": "..." }`) |
| `/api/wake` | POST | Wake the daemon (`{ "message": "..." }`) |
| `/api/events` | GET | SSE stream (events: `message`, `status`, `log`) |
