> **This app was imported here as a squashed subtree**, so `git log console/`
> starts at the import. The build is served by `cryohub` itself — set
> `console_dir` in `cryohub.toml` to this directory's `dist/` and there is no
> separate static file server to run.

# Agent Console

An installable phone app (Android + iOS) for reading and steering the AI agents
running in your [Chamber Hub](#chamber-hub). **One chamber = one project = one
flat conversation** — no topics to pick, no threads to lose track of.

It is a static site: a Vite + React build that talks to the hub's REST API on
its own origin. No backend of its own, no database, no telemetry.

<p align="center">
  <img src="docs/screenshots/projects.png" alt="Projects list with unread badges" width="260">
  <img src="docs/screenshots/conversation.png" alt="Conversation with chat bubbles and an attached plot" width="260">
  <img src="docs/screenshots/report.png" alt="A long agent report with markdown, display math and a code block" width="260">
</p>

## What it does

- **Projects list** — every chamber your token can reach, with unread counts and
  the last message as a preview. Hide the ones you do not care about in
  Settings.
- **One conversation per project** — opens on the newest message, follows new
  ones while you are at the bottom, and offers a jump chip instead of yanking
  the view when you have scrolled back.
- **Full markdown rendering** — tables, code blocks, LaTeX via KaTeX, emoji and
  @-mentions, rendered client-side and sanitized before it reaches the DOM. Long
  agent reports render as full-width cards so wide code and tables stay readable
  on a phone.
- **Composer** — auto-growing field, `@`-mention autocomplete, file upload, and
  Enter-to-send when a hardware keyboard is attached.
- **Attachments** — images and file links are fetched with your token, so
  chamber files display and download without a public URL.
- **Offline-tolerant** — an event loop with backoff and a reconnect notice that
  does not shove the page around. A message you send appears immediately and
  says whether it landed; a failed one waits in the thread for a tap to retry.
- **Drafts and dark mode** — a half-written message survives leaving the project
  or reloading, per project; the theme follows the system or your choice.
- **No accounts** — one owner token, and a friend gets in through an invite
  link scoped to the projects you tick.

There are **no push notifications** by design: the app only syncs while it is
open. It is a console you check, not a pager.

## Quick start

```sh
npm install
npm run dev        # http://localhost:5173 — /api proxies to 127.0.0.1:8765
npm test           # unit and component tests (Vitest)
npm run e2e        # Playwright end-to-end tests against a mocked hub
npm run build      # type-check, then emit static files to dist/
```

`npm run e2e` starts its own dev server on 5173. If that port is already taken —
another checkout, another worktree — set `E2E_PORT` so the suite tests *this*
tree rather than silently reusing whatever is already listening.

Sign in by pasting an access token: either the owner token the hub printed, or —
far more usually — by opening an invite link, which signs you in on the spot.

## Deploy

1. `npm run build` → static files in `dist/`.
2. Point `console_dir` in `cryohub.toml` at this directory's `dist/`, or copy it
   to the host and serve it yourself (see `deploy/Caddyfile`).
3. Install Caddy; copy `deploy/Caddyfile` to `/etc/caddy/Caddyfile`, set the
   real domain, and `systemctl reload caddy`. HTTPS is automatic.
4. Open the URL on a phone and use **Add to Home Screen**.

Caddy terminating TLS in front of the hub is not optional: `cryohub` stays bound
to loopback, and same-origin is what keeps the token out of any third-party
context.

## Chamber Hub

The app talks to a **Chamber Hub** (`cryohub`): your own machine's agent
chambers, served straight to a phone.

The thing that matters: a hub has **no accounts**. There is one owner token, and
everyone else gets an **invite link** scoped to the projects you tick. Opening
the link *is* signing in.

### Set it up

```sh
cryohub token owner          # prints the owner token — once. Keep it.
cryohub start --public       # listens on 127.0.0.1:8765
```

Point the app at it with a hub entry in `public/servers.json`:

```json
{ "name": "Chamber Hub", "prefix": "", "kind": "hub", "sendTopic": "" }
```

`prefix: ""` means "this origin": the hub is served from the same host as the
app, under `/api`. In dev, `npm run dev` proxies `/api` to `127.0.0.1:8765`. In
production the `agents.example.com` block in `deploy/Caddyfile` does the same —
copy it, replace the hostname, and make sure that name has a DNS record pointing
at the host **before** reloading Caddy, or no certificate can be issued.
`cryohub` itself stays bound to loopback; Caddy is the only way in.

`public/servers.json` normally holds exactly this one entry; with one entry the
login screen drops the server picker.

### Invite someone

1. Sign in on the hub server and paste the owner token into **Access token**.
2. *Settings → Share access* (owners only) → name the person, tick the projects
   they should see, **Create invite link**.
3. Send them the link. It is shown **once** — the hub stores only a hash, so it
   cannot be retrieved later. Lost link, new invite.
4. They open it on their phone, land straight in their projects, and can **Add
   to Home Screen** like any other install.

Revoke from the same screen. A revoked link stops working immediately: the next
thing that person's app does gets a 401 and drops them at the login screen with
*"This invite link is no longer valid."*

The token never appears in a URL the app requests — it rides in an
`Authorization: Bearer` header, and the `#invite=` fragment is stripped from the
address bar the moment it is read, before anything else runs.

## FAQ

**Why is a project missing from the list?**
The app shows the chambers the hub hands your token. Check the hub's scope for
that token, or *Settings → Projects* in case it is hidden.

**Can I use it on a desktop browser?**
Yes — the layout is phone-first but it works at any width, and Enter sends when
a hardware keyboard is present.

**What is stored on my phone?**
The access token, the name the hub knows you by, and a small cache of recent
messages, all in `localStorage`. Logging out clears them.

**Is message content from the server trusted?**
No. Markdown is rendered client-side and then sanitized before it reaches the
DOM: an allowlist of tags and attributes, inline styles filtered down to the
properties KaTeX needs (no URLs, no fixed positioning), and `url()` stripped
from SVG paint attributes. See `src/components/sanitize.ts`.

## Project layout

```
src/api/          Chamber Hub client, SSE reader, errors, types
src/store/        app state (Zustand), credential storage, local message cache
src/hooks/        the event loop (register + one SSE stream, with backoff)
src/components/   MessageBody + sanitizer, Composer, icon set
src/views/        Login, Projects, Conversation, Settings, Share
src/lib/          markdown+KaTeX renderer, theme, outbox, date/preview/colour helpers
src/styles.css    the design system (tokens first, then components, then dark)
e2e/              Playwright: smoke + layout contract, hub flows, screenshot harness
scripts/          icon generation from public/icons/icon.svg
```

## Regenerating the app icon

Edit `public/icons/icon.svg`, then:

```sh
node scripts/generate-icons.mjs   # rewrites icon-180/192/512.png
```

It renders through the Chromium that Playwright already installs, so there is no
extra dependency. The PNGs are committed, so a plain `npm run build` never needs
a browser.
