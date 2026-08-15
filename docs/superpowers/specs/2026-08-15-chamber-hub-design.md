# Chamber Hub: invite-link access to agents, replacing Zulip — Design

Date: 2026-08-15
Status: approved (this document records the design agreed in brainstorming)
Repos touched: `~/rcode/cryochamber` (hub server, Rust) and this repo (Agent Console PWA)

## Context

The Agent Console PWA currently talks to Zulip: one stream = one project, friends
need Zulip accounts, and the app proxies `qec-harness.zulipchat.com`. Two pain
points drove this design: account friction for invited friends, and Zulip's
constraints (rate limits, no cheap message-edit streaming, server-rendered HTML).

Investigation on the `qec` host showed the agent side is already
platform-agnostic:

- **chat-bridge** (`~/chat-bridge` on qec) syncs chat platforms to a chamber
  mailbox (`messages/inbox` / `messages/outbox`) behind a small `Channel`
  protocol, with Zulip and Lark adapters. One backbone can run several channels
  against one chamber simultaneously.
- **cryohub** (in the `cryochamber` crate, `src/hub/`) is an axum web server
  over the chamber root (`~/.cryo/chambers`): chamber discovery, mailbox-backed
  `GET /api/chambers/{id}/messages`, `POST /api/chambers/{id}/send` (writes to
  the chamber inbox and broadcasts on SSE), `GET /api/events` (SSE), todos,
  status, lifecycle (start/stop/restart/reset/archive), sync control, file
  watchers, host-header + CSRF security middleware. It is deliberately
  loopback-only with **no auth** today.

Conclusion: the "chamber server" is ~70% built. The gap is auth/invites,
scoping, attachments over HTTP, and a hub-speaking client in the PWA.

## Goals

- Friends need **no account**: opening an invite link is the whole onboarding.
- The owner can create, name, and revoke invite links at will.
- Invites are **scoped per project**: a link grants exactly the chambers it
  names, nothing else — including in the live event stream.
- The agent daemon keeps working unchanged; Zulip remains attached in parallel
  until the owner retires it.

## Non-goals

- Push notifications (owner preference: no noisy app).
- Reactions, threading, message editing UI (deferred; see Feature roadmap).
- Multi-owner / admin hierarchy. One owner token, N invite tokens.
- Federation or generic multi-server support for the hub client (the Zulip
  client keeps its generic `servers.json` behavior).

## Architecture

```
friend's phone ──HTTPS──> Caddy on qec ──loopback──> cryohub (axum, :8765)
   (PWA)                   - TLS                        - auth + scoping (NEW)
                           - serves PWA build           - mailbox read/write
                           - proxies /api/*             - SSE fan-out (filtered)
                                                        - attachments (NEW)
                                                            │
                                                   chamber dirs (~/.cryo/chambers)
                                                            │
                                              cryo daemon + chat-bridge (unchanged;
                                              Zulip channel stays during migration)
```

Auth lives **inside cryohub**, not in the proxy: the SSE stream must be
filtered per token scope, which a route-level proxy cannot do.

## 1. Access model

Two credential kinds, stored by the hub in `~/.config/cryo/cryohub-tokens.json`
(file mode 0600):

- **Owner token**: full API, including lifecycle, sync, and token management.
  Generated once via CLI (`cryohub token owner`), printed to the terminal.
- **Invite token**: `{ token, name, chambers: [chamber-id…], created_at,
  revoked_at? }`. Token is ≥32 bytes of CSPRNG output, URL-safe. Created,
  listed, and revoked via owner-only API (and a `cryohub token` CLI mirror).
  Revocation is a tombstone (`revoked_at`), not deletion, so the audit trail
  survives.

Invite link format: `https://<host>/#invite=<token>`. The secret travels in
the URL fragment — never sent to the server, never in access logs. The PWA
reads the fragment at boot, stores the token (localStorage, same trust level
as today's Zulip API key), and strips it from the URL.

Requests authenticate with `Authorization: Bearer <token>`.

Sender identity is **bound server-side**: a message sent with Alice's token is
stamped `from: Alice` by the hub, ignoring any client-supplied `from`. The
owner token sends as a configurable owner name (default `human`, matching the
current mailbox convention).

Comparison of one credential per link vs. accounts: a link *is* a bearer
credential — anyone it is forwarded to gains access. Accepted for a ~5-friend
trust circle; mitigations are per-friend links (blast radius = one identity),
easy revocation, fragment transport, and per-project scoping.

## 2. Authorization matrix

| Route | Owner | Invite (in scope) | Invite (out of scope) | No token |
|---|---|---|---|---|
| `GET /api/chambers` | all chambers | filtered to scope | — | 401 |
| `GET /api/chambers/{id}/messages` | ✓ | ✓ | 404 | 401 |
| `POST /api/chambers/{id}/send` | ✓ (as owner name) | ✓ (as invite name) | 404 | 401 |
| `GET /api/chambers/{id}/status`, `/todos` | ✓ | ✓ | 404 | 401 |
| attachments up/download (new, §3) | ✓ | ✓ | 404 | 401 |
| `GET /api/events` (SSE) | all events | events for scoped chambers only | n/a (filtered) | 401 |
| lifecycle (`start/stop/restart/reset/archive`), sync routes | ✓ | 403 | 403 | 401 |
| token management (new) | ✓ | 403 | 403 | 401 |
| static PWA assets, `/`, `/c/{id}` | public | public | public | public |

Out-of-scope chambers return **404, not 403** — an invite must not be able to
enumerate chamber ids it wasn't given.

**Public mode**: auth is enforced when the hub runs with `--public` (or a
config flag). The current loopback no-auth behavior is preserved for local
use; `--public` refuses to start if no owner token exists. The existing
host-header allowlist and CSRF middleware remain in force.

## 3. Attachments

- `POST /api/chambers/{id}/uploads` — multipart, 25 MB cap (matching
  chat-bridge's `MAX_ATTACHMENT_BYTES`). Saves under the chamber's attachment
  directory (same convention chat-bridge uses when downloading platform
  attachments), filename sanitized + content-hash prefixed to prevent
  collisions and traversal. Returns a markdown link
  `[name](/api/chambers/{id}/files/<file>)` ready to embed in a message.
- `GET /api/chambers/{id}/files/<file>` — authed, scope-checked, serves with a
  correct Content-Type and `Content-Disposition`. Path resolution must stay
  inside the chamber (reuse the `resolve_local_link`-style containment check).

## 4. PWA changes (this repo)

- **`HubClient`** implementing the same interface the app consumes from
  `client.ts` today: list projects (= scoped chambers), fetch messages, send,
  attachments, and a live-events subscription. Selected per entry in
  `servers.json` (`kind: "hub" | "zulip"`); the Zulip client stays fully
  functional until retired.
- **SSE via fetch-streaming reader**, not `EventSource` (which cannot set an
  `Authorization` header — the token must never ride in a query string).
  Reconnect with backoff; on reconnect, re-fetch messages to close gaps
  (the store's existing gap-handling merge applies).
- **Onboarding**: at boot, `#invite=<token>` (or an owner token pasted into a
  field) is stored and the fragment removed via `history.replaceState`; the
  user lands directly on the project list. Invalid/revoked token → the login
  screen with a clear reason (existing `loginReason` mechanism).
- **Markdown rendering**: hub messages are raw markdown (Zulip served rendered
  HTML). Add client-side rendering — markdown-it (CommonMark + tables +
  fenced code) plus the existing KaTeX pipeline — feeding the **existing
  DOMPurify sanitizer**, which stays the single choke point for HTML entering
  the DOM. All sanitizer hardening and tests carry over.
- **Share screen** (owner token only): per-project list of invite links with
  name, created date, scope; create (name + project picker) and revoke
  buttons; copy-link action producing the `#invite=` URL.
- Local message cache, WeChat rendering, mentions UI, lightbox, upload UI all
  carry over unchanged. Mention *autocomplete* on hub projects derives its
  name list from senders seen in the loaded messages — no new endpoint in v1.

## 5. Deployment & migration

- cryohub binds `127.0.0.1:8765` in public mode too; **Caddy** terminates TLS,
  serves the PWA build, and reverse-proxies `/api/*` to the hub. No open
  relay; only the hub upstream.
- **Prerequisite (open)**: public reachability for qec — either a DNS name
  with 80/443 open (Caddy auto-TLS), or a Tailscale/Cloudflare-Tunnel style
  private ingress. Decision pending; nothing else in the design depends on
  which.
- **Migration**: chat-bridge's Zulip channel stays attached to the same
  chambers throughout. Friends move by link; message history starts fresh on
  the hub side (the mailbox is the source of truth; no Zulip history import in
  v1). Zulip is turned off manually once the hub path has proven itself.

## 6. Feature roadmap (chat-UX, re-based onto the hub)

Ships with v1: send states (sending / sent / failed-with-retry), per-project
drafts, copy button on code blocks, dark mode (tokens exist).
Next: live-updating agent replies (streaming edits — now a hub/daemon feature
we control: an outbox convention plus an SSE `MessageUpdated` event), reaction
shorthand, in-conversation search.
Explicitly out: push notifications.

## Error handling

- 401 → token invalid/revoked → PWA clears stored token, shows login screen
  with reason (mirrors today's Zulip 401 behavior).
- 404 on a previously-visible chamber → treat as revoked scope: drop it from
  the project list.
- SSE drop → reconnect with exponential backoff; visible "Reconnecting" banner
  (existing component); gap re-fetch on reconnect.
- Upload failures surface inline (existing composer error path).

## Testing

- **Rust (cryochamber)**: integration tests for the full authorization matrix
  (every row above), token lifecycle (create/revoke/tombstone), SSE filtering
  (invite receives scoped events only — the security-critical test), attachment
  containment (traversal attempts 404), and public-mode startup refusing to run
  without an owner token.
- **PWA (vitest)**: `HubClient` unit tests against a mocked fetch (messages,
  send, SSE reader parsing, reconnect/backoff), invite onboarding (fragment
  consumed, token stored, URL cleaned), markdown pipeline (markdown-it output
  passes through the sanitizer; hostile-markdown cases), Share screen flows.
- **e2e (Playwright)**: against a mock hub server: open invite link → see
  scoped project only → send → receive via SSE → revoked token → login screen.

## Open questions

1. Public ingress for qec (DNS + 443 vs. tunnel) — deployment prerequisite,
   does not block implementation.
