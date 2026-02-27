# Zulip Message Channel Design

## Summary

Implement a Zulip bot backend for the `MessageChannel` trait, alongside the existing GitHub Discussions and file-based channels. A separate `cryo-zulip` binary syncs messages between a Zulip stream and the local `messages/inbox/` and `messages/outbox/` directories.

## Architecture

Mirrors `cryo-gh` exactly:

```
Zulip Stream
    ↓ (cryo-zulip sync-daemon polls GET /messages)
messages/inbox/
    ↓ (cryo daemon watches)
Agent reads on wake
    ↓ (cryo-agent send)
messages/outbox/
    ↓ (cryo-zulip sync-daemon watches)
Zulip Stream (POST /messages)
```

The cryo daemon and agent remain unaware of Zulip. All sync is handled externally by the `cryo-zulip` daemon.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Binary structure | Separate `cryo-zulip` binary | Mirrors cryo-gh, keeps Zulip optional |
| Zulip entity mapping | Entire stream per project | Dedicated stream, all messages synced |
| Topic handling | Fixed topic per project for outgoing | Default "cryochamber", configurable at init. Pull fetches all stream messages regardless of topic |
| Authentication | `zuliprc` file (INI format) | Standard Zulip auth, same as ZulipCourseBot |
| Message fetching | REST polling (`GET /messages`) | Stateless, handles daemon restarts, mirrors cryo-gh interval pattern |
| HTTP client | `ureq` (blocking) | Lightweight, no async runtime, sufficient for simple REST calls |
| API module structure | `ZulipClient` struct | Encapsulates credentials + HTTP agent, clean Rust pattern |
| Message filtering | All stream messages, skip self-authored | Broad inbox, self-filter by bot email |

## New Files

| File | Purpose |
|------|---------|
| `src/channel/zulip.rs` | `ZulipClient` struct + REST API methods |
| `src/bin/cryo_zulip.rs` | CLI binary (init/pull/push/sync/unsync/status) |
| `src/zulip_sync.rs` | `ZulipSyncState` persistence (`zulip-sync.json`) |
| `tests/zulip_channel_tests.rs` | Unit tests for API parsing |
| `tests/zulip_sync_tests.rs` | Sync state serialization tests |

## Modified Files

| File | Change |
|------|--------|
| `Cargo.toml` | Add `ureq` dependency, `cryo-zulip` binary entry |
| `src/lib.rs` | Add `pub mod zulip_sync` |
| `src/channel/mod.rs` | Add `pub mod zulip` |
| `src/bin/cryo.rs` | Add `zulip-sync` service cleanup in `cryo clean` |

## Component Details

### ZulipClient (`src/channel/zulip.rs`)

```rust
pub struct ZulipCredentials {
    pub email: String,
    pub api_key: String,
    pub site: String,  // e.g. "https://zulip.example.com"
}

pub struct ZulipClient {
    creds: ZulipCredentials,
    agent: ureq::Agent,
}
```

Methods:

| Method | Zulip API Endpoint | Purpose |
|--------|--------------------|---------|
| `from_zuliprc(path)` | — | Parse INI-format zuliprc file |
| `get_profile()` | `GET /api/v1/users/me` | Get bot identity (user_id, email) |
| `get_stream_id(name)` | `GET /api/v1/get_stream_id` | Resolve stream name → numeric ID |
| `get_messages(stream_id, anchor, num_after)` | `GET /api/v1/messages` | Fetch messages since anchor |
| `send_message(stream_id, topic, content)` | `POST /api/v1/messages` | Post message to stream + topic |

Auth: HTTP Basic Auth (`email:api_key`) on every request.

Message fetching uses `narrow=[{"operator":"stream","operand":stream_id}]`, `anchor=last_message_id` (or `"oldest"` for first pull), `num_after=1000`. Paginates until no more messages.

### ZulipSyncState (`src/zulip_sync.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZulipSyncState {
    pub site: String,
    pub stream: String,
    pub stream_id: u64,
    pub self_email: String,
    #[serde(default)]
    pub topic: Option<String>,           // default "cryochamber"
    #[serde(default)]
    pub last_message_id: Option<u64>,
    #[serde(default)]
    pub last_pushed_session: Option<u32>,
}
```

Persisted as `zulip-sync.json`. Uses `#[serde(default)]` for backward compatibility.

### CLI Commands (`src/bin/cryo_zulip.rs`)

| Command | Behavior |
|---------|----------|
| `cryo-zulip init --config zuliprc --stream name [--topic name]` | Validate credentials, resolve stream ID, get bot email, save zulip-sync.json |
| `cryo-zulip pull` | GET /messages since last anchor → write to `messages/inbox/` as markdown files |
| `cryo-zulip push` | Push latest session log → Zulip stream (to configured topic) |
| `cryo-zulip sync [--interval 30]` | Install OS service for sync daemon |
| `cryo-zulip unsync` | Uninstall sync service |
| `cryo-zulip status` | Display zulip-sync.json contents |
| `cryo-zulip sync-daemon --interval N` | (hidden) Actual sync loop |

### Sync Daemon Loop

1. Register signal handlers (SIGTERM, SIGINT)
2. Watch `messages/outbox/` for new files via `notify`
3. Main loop each interval:
   - Reload sync state (cursor may have been updated by manual `pull`)
   - Pull: `GET /messages` with anchor → write new messages to inbox
   - Push outbox: read `messages/outbox/*.md` → `POST /messages` → archive
4. On outbox file event: immediate push cycle

### Push Format

- **Session push**: `## Session {N}\n\n{session_content}`
- **Outbox push**: `**{from}** ({subject})\n\n{body}`
- **Topic**: Value from `zulip-sync.json` (default `"cryochamber"`)

### Message Conversion (Zulip → Inbox)

Zulip message JSON → `Message` struct:
- `from`: `sender_full_name`
- `subject`: Zulip `subject` field (topic name)
- `body`: `content` field (Zulip markdown)
- `timestamp`: from Unix `timestamp` field
- `metadata`: `source: "zulip"`, `zulip_message_id: "{id}"`

Written to `messages/inbox/{timestamp}_{slug}.md` with YAML frontmatter.

## Error Handling

- **Auth failure**: Clear error pointing to zuliprc path
- **Stream not found**: Error on init with stream name
- **Network errors**: Log and continue, daemon retries next interval
- **Rate limiting (429)**: Respect `Retry-After` header

## Testing

- **Unit tests** (`tests/zulip_channel_tests.rs`): Parse zuliprc, parse message JSON response, build request parameters, self-message filtering
- **Sync state tests** (`tests/zulip_sync_tests.rs`): Roundtrip serialization, missing file, backward compatibility
- **No live API tests**: Same pattern as github_channel_tests — test parsing and construction, not network calls

## Reference

Design informed by [ZulipCourseBot](~/pycode/ZulipCourseBot) Python implementation which uses the Zulip Python SDK. Key differences:
- ZulipCourseBot uses event queue long-polling; we use REST polling for simplicity and restart resilience
- ZulipCourseBot is a real-time bot; we're a periodic sync daemon
- We use `ureq` for direct HTTP instead of a Zulip SDK (no Rust SDK exists with sufficient maturity)
