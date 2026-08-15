# Zulip Agent PWA — Design Spec

**Date:** 2026-08-14
**Status:** Approved design, pending implementation plan

## Purpose

A phone-friendly frontend for the owner and a small group of friends (~5 users,
Android and iOS) to remote-control AI agents running on a remote host. The
agents are driven by an existing, generic daemon that reacts to messages in
Zulip streams — one stream per project. The app is generic over Zulip servers:
the initial deployment targets `qec-harness.zulipchat.com` (Zulip Cloud), and
additional servers can be added by configuration alone. This app replaces the
hard-to-use official Zulip client with a minimal, focused UI.

**Build-vs-buy verdict (from brainstorm):** keep Zulip as the backbone (auth,
multi-user permissions, history, sync, real-time API); build only a thin web
frontend. No host-side agent code is in scope — the daemon already exists.

## Non-goals

- No push notifications of any kind (owner's explicit preference: quiet,
  pull-based app). Nothing runs when the app is closed.
- No topic/task abstraction in the UI — one stream = one project = one flat
  conversation.
- No offline composing, no file/image upload in v1 (attachments *display*
  via proxied links).
- No admin features: accounts, stream subscriptions, and permissions are
  managed on Zulip's web UI.
- No native app, no app stores.

## Architecture

```
Phone (Android/iOS browser or installed PWA)
  └─ Static PWA  (Vite + React + TypeScript, no server-side logic)
       │  same-origin HTTPS
       ▼
Remote host: Caddy web server
  ├─ serves the PWA's static files (incl. servers.json)
  └─ reverse-proxies, per allowlisted server:
       /zulip/qec/* ──▶ https://qec-harness.zulipchat.com/*
       /zulip/<key>/* ──▶ https://<other-allowlisted-server>/*
```

1. **PWA** — static single-page app. Installable via web manifest + minimal
   service worker (app-shell caching only). All state lives in the browser:
   the user's email + Zulip API key in `localStorage`; message data in memory.
2. **Caddy** — configuration, not code. Serves static files and proxies
   `/zulip/<key>/*` to the corresponding Zulip server from a fixed
   **allowlist** in the Caddyfile. This is what makes browser → Zulip calls
   possible: Zulip servers (including Zulip Cloud) send no CORS headers for
   foreign origins, so the app and the API must share one origin. The proxy
   is deliberately *not* an open relay — only allowlisted upstreams are
   reachable, so the host cannot be abused as a forwarding proxy. Caddy also
   provides automatic HTTPS. Adding a server = one Caddyfile line + one
   `servers.json` entry; no app rebuild.
3. **`servers.json`** — a static config file deployed next to the app:
   `[{ "name": "QEC Harness", "prefix": "/zulip/qec" }, …]`. The app loads it
   at startup to populate the server picker.
4. **Zulip server(s)** — untouched. The proxy relays credentials without
   storing them; no component of this project ever persists a user's password
   or API key server-side.

### Rendering strategy

The Zulip API returns messages as **server-rendered HTML** (markdown, code
with pygments highlighting, KaTeX math — already rendered). The app sanitizes
this HTML with DOMPurify (fixed allowlist) and styles it with Zulip's KaTeX +
pygments CSS shipped as local assets. No client-side markdown or math renderer
is implemented.

Relative URLs inside rendered HTML (e.g. `/user_uploads/…` attachment and
image links) are rewritten by `MessageBody` to the active server's proxy
prefix so they resolve through the same origin.

### Auth

- Login form: server (picked from `servers.json`; picker hidden when only one
  entry) + email + password → `POST <prefix>/api/v1/fetch_api_key` (through
  the proxy) → store server prefix + email + API key in `localStorage`.
  Password is never stored.
- Fallback: "paste API key instead" link for anyone whose password login
  misbehaves.
- All subsequent requests use HTTP basic auth (email + API key), Zulip's
  standard scheme.

## Screens

Flat, two-level navigation plus login and settings:

1. **Login** — server picker (from `servers.json`, hidden if single entry),
   email + password form; paste-API-key fallback.
2. **Projects** (home) — subscribed streams as cards: name, description,
   unread count. Per-stream hide toggle (client-side preference in
   `localStorage`, managed in Settings) so each user curates their own list.
3. **Conversation** (per project) — all messages in the stream as one
   chronological timeline, ignoring topic boundaries. Sender name, timestamp,
   sanitized Zulip HTML body. Composer pinned at bottom: plain
   text/markdown, send button. A "Load earlier" control at the top loads
   older history.
4. **Settings** (sheet) — logged-in identity, hidden-streams management,
   log out.

### The fixed-topic rule

Zulip protocol requires every stream message to carry a topic. The composer
always sends to Zulip's "general chat" empty topic; if the server version
predates empty-topic support, a config constant (default `"chat"`) is used
instead. Assumption (owner-confirmed design): the daemon does not route on
topic names, so this is invisible to it. Messages arriving in other topics
(e.g. from desktop sessions) still appear in the timeline.

## Components

- **API client module** — auth, message fetch/pagination, send, event queue
  lifecycle, read flags. The only place that knows the Zulip protocol.
- **Store** — streams, messages, unread counts (always derived from Zulip's
  flags, never local bookkeeping), hidden-stream prefs.
- **Route views** — Login, Projects, Conversation, Settings.
- **`MessageBody`** — shared sanitize-and-style renderer for Zulip HTML.

## Data flow

- **Cold start:** one `register` call creates an event queue and returns the
  initial snapshot (streams, unreads). Conversation history lazy-loads via
  `GET /messages` narrowed to the stream, in pages of ~50. Hidden streams are
  never fetched.
- **Live updates (foreground only):** long-poll `GET /events`. New messages
  render in real time in the open conversation and bump unread badges
  elsewhere. Backgrounding pauses polling; foregrounding resumes it, and an
  expired queue (Zulip GC's idle queues after ~10 min) triggers transparent
  re-register + re-sync of the visible conversation.
- **Sending:** `POST /messages` (stream + fixed topic + content). Rendered
  when it echoes back through the event stream — no optimistic rendering in
  v1. On failure the composer keeps the text and offers retry.
- **Read state:** viewing a conversation marks the stream read in Zulip.
  Phone, desktop `zlp` mirror, and web client always agree on unreads.

## Error handling

- **401 anywhere** → login screen with reason shown. Stored key cleared only
  on explicit logout or confirmed-invalid key, never on transient errors.
- **Network loss:** event long-poll retries with exponential backoff; a
  "reconnecting…" banner shows while live updates are stalled (silence must
  never look like "no agent activity"). Failed sends keep composer text and
  offer retry.
- **Expired event queue:** transparent re-register + re-fetch; at most a
  brief visible refresh.
- **Proxy down vs Zulip down:** indistinguishable and treated identically —
  reconnect banner + retry. No stack traces shown to users; errors go to the
  console.
- **Renderer safety:** all message HTML passes DOMPurify with a fixed
  allowlist before touching the DOM.

## Testing

- **Unit (bulk):** API client against mocked `fetch` — login/key fetch,
  pagination, fixed-topic send, and the event-queue lifecycle (expiry →
  re-register → re-sync). Store tests for unread derivation and stream
  hiding. Vitest.
- **Rendering:** `MessageBody` against fixture HTML captured from the real
  server — code blocks, KaTeX, links, quotes, attachments, plus hostile
  payloads DOMPurify must strip.
- **E2E:** one Playwright smoke (login → projects → conversation → send)
  against a mocked API layer, in GitHub Actions on every push.
- **Manual per-release device checklist:** home-screen install on one Android
  and one iPhone; safe-area rendering; keyboard-over-composer; background →
  foreground resume.
- TDD during implementation.

## Deployment

- Build output is static files; deploy = copy `dist/` to the remote host.
- Caddyfile (versioned in this repo under `deploy/`) defines the static site,
  the per-server `/zulip/<key>/*` reverse-proxy allowlist, and automatic
  HTTPS. Requires a DNS name pointing at the host. `servers.json` is deployed
  alongside `dist/` and must stay in sync with the Caddyfile allowlist.
- Friends onboard by opening the URL, logging in, and "Add to Home Screen."
