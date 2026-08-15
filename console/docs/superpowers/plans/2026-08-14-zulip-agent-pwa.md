# Zulip Agent PWA Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A static, installable PWA that lets ~5 users on Android/iOS read and send messages in Zulip project streams (one stream = one project = one flat conversation), served by Caddy which also reverse-proxies allowlisted Zulip servers under `/zulip/<key>/*`.

**Architecture:** All logic runs in the browser; the only server piece is a Caddyfile (static files + per-server proxy allowlist). Auth = email+password → Zulip `fetch_api_key`, stored in `localStorage`. Live updates via Zulip's long-poll events API, foreground only. Message bodies are Zulip's server-rendered HTML, sanitized with DOMPurify.

**Tech Stack:** Vite + React 19 + TypeScript (strict), zustand, DOMPurify, KaTeX CSS, Vitest + Testing Library, Playwright, Caddy.

**Spec:** `docs/superpowers/specs/2026-08-14-zulip-agent-pwa-design.md`

## Global Constraints

- Node ≥ 20; npm as package manager; all commands run from repo root.
- TypeScript `strict: true`; `npm run build` must pass `tsc --noEmit`.
- Runtime dependencies EXACTLY: `react`, `react-dom`, `zustand`, `dompurify`, `katex`. Dev-only additions require a spec change.
- Every Zulip API call goes through the active server's proxy prefix (e.g. `/zulip/qec`); NO absolute Zulip URLs anywhere in `src/`.
- No push notifications; no timers/pollers run when the app is closed or logged out.
- The composer's send topic comes from the server config `sendTopic` (default `""` = Zulip "general chat" empty topic).
- `localStorage` keys are namespaced `zulip-app.*`.
- Default server: name `QEC Harness`, prefix `/zulip/qec`, upstream `https://qec-harness.zulipchat.com`.
- All UI copy in English. Commit after every task (each task's last step is a commit).

---

## File Structure

```
zulip-app/
├── package.json / tsconfig.json / vite.config.ts / index.html / .gitignore
├── public/
│   ├── servers.json            # server allowlist shown in login picker
│   ├── manifest.webmanifest    # PWA manifest (Task 14)
│   ├── sw.js                   # minimal service worker (Task 14)
│   └── icons/                  # generated PNGs (Task 14)
├── src/
│   ├── main.tsx                # bootstrap, global CSS, SW registration
│   ├── App.tsx                 # auth guard + view switch + banner
│   ├── styles.css              # the app's single stylesheet
│   ├── api/
│   │   ├── types.ts            # all shared types + event type-guards
│   │   ├── servers.ts          # loadServers() from /servers.json
│   │   └── client.ts           # ZulipClient — the ONLY Zulip-protocol code
│   ├── store/
│   │   ├── auth.ts             # credentials persistence (localStorage)
│   │   └── appStore.ts         # zustand store: data + navigation + prefs
│   ├── components/
│   │   ├── MessageBody.tsx     # sanitize + URL-rewrite + render Zulip HTML
│   │   └── Composer.tsx        # text box + send/retry
│   ├── views/
│   │   ├── LoginView.tsx
│   │   ├── ProjectsView.tsx
│   │   ├── ConversationView.tsx
│   │   └── SettingsSheet.tsx
│   ├── hooks/
│   │   └── useEventLoop.ts     # register/poll/backoff/visibility lifecycle
│   └── test/
│       ├── setup.ts
│       └── fixtures/zulipHtml.ts
├── e2e/smoke.spec.ts + playwright.config.ts
├── deploy/Caddyfile
├── .github/workflows/ci.yml
└── README.md
```

---

### Task 1: Project scaffold (Vite + React + TS + Vitest)

**Files:**
- Create: `package.json`, `tsconfig.json`, `vite.config.ts`, `index.html`, `.gitignore`, `src/main.tsx`, `src/App.tsx`, `src/styles.css`, `src/test/setup.ts`
- Test: `src/App.test.tsx`

**Interfaces:**
- Consumes: nothing.
- Produces: a running Vitest setup (jsdom, globals, jest-dom matchers); `src/styles.css` class names used by all views (`topbar`, `stream-list`, `stream-card`, `unread-badge`, `conversation`, `message`, `message-body`, `composer`, `banner`, `sheet`, `login`, `empty`, `error`, `link`, `danger`); placeholder `App` (replaced in Task 13).

- [ ] **Step 1: Write config + scaffold files**

`package.json`:

```json
{
  "name": "zulip-agent-pwa",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "preview": "vite preview",
    "test": "vitest run",
    "test:watch": "vitest",
    "e2e": "playwright test"
  },
  "dependencies": {
    "dompurify": "^3.2.4",
    "katex": "^0.16.21",
    "react": "^19.1.0",
    "react-dom": "^19.1.0",
    "zustand": "^5.0.3"
  },
  "devDependencies": {
    "@playwright/test": "^1.53.0",
    "@testing-library/jest-dom": "^6.6.3",
    "@testing-library/react": "^16.3.0",
    "@testing-library/user-event": "^14.6.1",
    "@types/react": "^19.1.0",
    "@types/react-dom": "^19.1.0",
    "@vitejs/plugin-react": "^4.5.0",
    "jsdom": "^26.1.0",
    "typescript": "~5.8.3",
    "vite": "^7.0.0",
    "vitest": "^3.2.0"
  }
}
```

`tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "skipLibCheck": true,
    "noEmit": true,
    "types": ["vitest/globals", "@testing-library/jest-dom"]
  },
  "include": ["src"]
}
```

`vite.config.ts` (the dev proxy mirrors production Caddy so `npm run dev` works against the real server):

```ts
/// <reference types="vitest/config" />
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/zulip/qec': {
        target: 'https://qec-harness.zulipchat.com',
        changeOrigin: true,
        rewrite: (p) => p.replace(/^\/zulip\/qec/, ''),
      },
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: './src/test/setup.ts',
    exclude: ['e2e/**', 'node_modules/**'],
  },
})
```

`index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover" />
    <title>Agent Console</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

`.gitignore`:

```
node_modules/
dist/
test-results/
playwright-report/
```

`src/main.tsx`:

```tsx
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import 'katex/dist/katex.min.css'
import './styles.css'
import App from './App'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
```

`src/App.tsx` (placeholder — fully replaced in Task 13):

```tsx
export default function App() {
  return <h1>Agent Console</h1>
}
```

`src/test/setup.ts`:

```ts
import '@testing-library/jest-dom/vitest'

afterEach(() => {
  localStorage.clear()
})
```

`src/styles.css` (complete stylesheet; later tasks only consume these classes):

```css
* { box-sizing: border-box; }
html, body, #root { height: 100%; margin: 0; }
body {
  font: 16px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  background: #f8fafc; color: #111827;
}
button { font: inherit; cursor: pointer; }
.app { display: flex; flex-direction: column; height: 100%; }

.topbar {
  display: flex; align-items: center; gap: 0.5rem;
  padding: 0.75rem 1rem; padding-top: calc(0.75rem + env(safe-area-inset-top));
  background: #4f46e5; color: #fff; position: sticky; top: 0;
}
.topbar h1, .topbar h2 { font-size: 1.1rem; margin: 0; flex: 1; }
.topbar button { background: none; border: none; color: #fff; font-size: 1.2rem; }

.banner {
  background: #b45309; color: #fff; text-align: center;
  padding: 0.25rem; font-size: 0.85rem;
}

.stream-list { list-style: none; margin: 0; padding: 0.5rem; }
.stream-card {
  display: grid; grid-template-columns: 1fr auto; gap: 0.25rem;
  width: 100%; text-align: left; margin-bottom: 0.5rem; padding: 0.75rem 1rem;
  background: #fff; border: 1px solid #e5e7eb; border-radius: 0.75rem;
}
.stream-name { font-weight: 600; }
.stream-desc { grid-column: 1 / -1; color: #6b7280; font-size: 0.85rem; }
.unread-badge {
  background: #4f46e5; color: #fff; border-radius: 999px;
  padding: 0 0.5rem; font-size: 0.8rem; align-self: center;
}

.conversation { display: flex; flex-direction: column; flex: 1; min-height: 0; }
.message-scroll { flex: 1; overflow-y: auto; padding: 0.5rem 1rem; }
.message { margin-bottom: 0.75rem; }
.message-meta { font-size: 0.8rem; color: #6b7280; }
.message-meta .sender { font-weight: 600; color: #111827; }
.message-body { overflow-wrap: break-word; }
.message-body img { max-width: 100%; height: auto; }
.message-body pre { overflow-x: auto; background: #f3f4f6; padding: 0.5rem; border-radius: 0.5rem; }
.message-body code { background: #f3f4f6; border-radius: 0.25rem; padding: 0 0.2rem; }
.message-body blockquote { border-left: 3px solid #d1d5db; margin: 0.25rem 0; padding-left: 0.75rem; color: #4b5563; }

/* Minimal pygments palette for Zulip's .codehilite spans */
.codehilite .k, .codehilite .kn, .codehilite .kd { color: #7c3aed; }
.codehilite .s, .codehilite .s1, .codehilite .s2, .codehilite .sd { color: #b45309; }
.codehilite .nf, .codehilite .nc { color: #1d4ed8; }
.codehilite .c, .codehilite .c1, .codehilite .cm { color: #6b7280; font-style: italic; }
.codehilite .mi, .codehilite .mf { color: #047857; }
.codehilite .o, .codehilite .p { color: #374151; }

.composer {
  display: flex; gap: 0.5rem; padding: 0.5rem 1rem;
  padding-bottom: calc(0.5rem + env(safe-area-inset-bottom));
  border-top: 1px solid #e5e7eb; background: #fff;
}
.composer textarea { flex: 1; resize: none; border: 1px solid #d1d5db; border-radius: 0.5rem; padding: 0.5rem; font: inherit; }
.composer button { background: #4f46e5; color: #fff; border: none; border-radius: 0.5rem; padding: 0 1rem; }
.composer button:disabled { opacity: 0.5; }

.login { max-width: 22rem; margin: 10vh auto; display: flex; flex-direction: column; gap: 0.75rem; padding: 1rem; }
.login label { display: flex; flex-direction: column; gap: 0.25rem; font-size: 0.9rem; }
.login input, .login select { padding: 0.5rem; border: 1px solid #d1d5db; border-radius: 0.5rem; font: inherit; }
.login button[type="submit"] { background: #4f46e5; color: #fff; border: none; border-radius: 0.5rem; padding: 0.6rem; }

.sheet {
  position: fixed; inset: 0; background: #fff; z-index: 10;
  display: flex; flex-direction: column; gap: 0.5rem; overflow-y: auto;
}
.sheet ul { list-style: none; padding: 0 1rem; margin: 0; }
.sheet li { padding: 0.4rem 0; }
.sheet h3, .sheet .identity { padding: 0 1rem; }

.empty { color: #6b7280; text-align: center; padding: 2rem 1rem; }
.error { color: #b91c1c; font-size: 0.9rem; }
.link { background: none; border: none; color: #4f46e5; text-decoration: underline; padding: 0; }
.danger { margin: 1rem; padding: 0.6rem; background: #fff; color: #b91c1c; border: 1px solid #b91c1c; border-radius: 0.5rem; }
.load-earlier { margin: 0.5rem auto; display: block; background: none; border: 1px solid #d1d5db; border-radius: 999px; padding: 0.25rem 1rem; color: #4b5563; }
```

- [ ] **Step 2: Write the failing test**

`src/App.test.tsx`:

```tsx
import { render, screen } from '@testing-library/react'
import App from './App'

test('renders the app title', () => {
  render(<App />)
  expect(screen.getByRole('heading', { name: 'Agent Console' })).toBeInTheDocument()
})
```

- [ ] **Step 3: Install and run test to verify it passes**

Run: `npm install && npm test`
Expected: 1 test PASS. (Scaffold task: the "failing first" cycle starts in Task 2; here the test validates the toolchain.)

- [ ] **Step 4: Verify the build works**

Run: `npm run build`
Expected: `tsc` silent, Vite writes `dist/`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: scaffold Vite + React + TS + Vitest PWA project"
```

---

### Task 2: Shared types + credentials storage

**Files:**
- Create: `src/api/types.ts`, `src/store/auth.ts`
- Test: `src/store/auth.test.ts`

**Interfaces:**
- Consumes: nothing.
- Produces (used verbatim by all later tasks):
  - `ServerConfig { name: string; prefix: string; sendTopic?: string }`
  - `Credentials { prefix: string; email: string; apiKey: string; sendTopic: string }`
  - `ZulipMessage { id: number; sender_full_name: string; sender_email: string; timestamp: number; content: string; stream_id: number; subject: string }`
  - `StreamSub { stream_id: number; name: string; description: string }`
  - `UnreadStreamEntry { stream_id: number; topic: string; unread_message_ids: number[] }`
  - `InitialState { queueId: string; lastEventId: number; subscriptions: StreamSub[]; unread: UnreadStreamEntry[] }`
  - `ZulipEvent` union + guards `isMessageEvent(ev)`, `isReadFlagsEvent(ev)`
  - `saveCredentials(c: Credentials): void`, `loadCredentials(): Credentials | null`, `clearCredentials(): void`

- [ ] **Step 1: Write the types file**

`src/api/types.ts`:

```ts
export interface ServerConfig {
  name: string
  prefix: string
  sendTopic?: string
}

export interface Credentials {
  prefix: string
  email: string
  apiKey: string
  sendTopic: string
}

export interface ZulipMessage {
  id: number
  sender_full_name: string
  sender_email: string
  timestamp: number
  content: string // server-rendered HTML
  stream_id: number
  subject: string
}

export interface StreamSub {
  stream_id: number
  name: string
  description: string
}

export interface UnreadStreamEntry {
  stream_id: number
  topic: string
  unread_message_ids: number[]
}

export interface InitialState {
  queueId: string
  lastEventId: number
  subscriptions: StreamSub[]
  unread: UnreadStreamEntry[]
}

export interface MessageEvent {
  id: number
  type: 'message'
  message: ZulipMessage
}

export interface FlagsEvent {
  id: number
  type: 'update_message_flags'
  flag: string
  op?: 'add' | 'remove'
  operation?: 'add' | 'remove' // older Zulip servers use this field name
  messages: number[]
}

export type ZulipEvent = MessageEvent | FlagsEvent | { id: number; type: string }

export function isMessageEvent(ev: ZulipEvent): ev is MessageEvent {
  return ev.type === 'message'
}

export function isReadFlagsEvent(ev: ZulipEvent): ev is FlagsEvent {
  return ev.type === 'update_message_flags' && (ev as FlagsEvent).flag === 'read'
}
```

- [ ] **Step 2: Write the failing test**

`src/store/auth.test.ts`:

```ts
import { saveCredentials, loadCredentials, clearCredentials } from './auth'
import type { Credentials } from '../api/types'

const creds: Credentials = {
  prefix: '/zulip/qec',
  email: 'a@b.c',
  apiKey: 'secret',
  sendTopic: '',
}

test('round-trips credentials through localStorage', () => {
  saveCredentials(creds)
  expect(loadCredentials()).toEqual(creds)
})

test('returns null when nothing stored', () => {
  expect(loadCredentials()).toBeNull()
})

test('returns null on corrupt stored JSON', () => {
  localStorage.setItem('zulip-app.credentials', '{not json')
  expect(loadCredentials()).toBeNull()
})

test('clearCredentials removes stored value', () => {
  saveCredentials(creds)
  clearCredentials()
  expect(loadCredentials()).toBeNull()
})
```

- [ ] **Step 3: Run test to verify it fails**

Run: `npm test`
Expected: FAIL — cannot resolve `./auth`.

- [ ] **Step 4: Write the implementation**

`src/store/auth.ts`:

```ts
import type { Credentials } from '../api/types'

const KEY = 'zulip-app.credentials'

export function saveCredentials(c: Credentials): void {
  localStorage.setItem(KEY, JSON.stringify(c))
}

export function loadCredentials(): Credentials | null {
  const raw = localStorage.getItem(KEY)
  if (!raw) return null
  try {
    return JSON.parse(raw) as Credentials
  } catch {
    return null
  }
}

export function clearCredentials(): void {
  localStorage.removeItem(KEY)
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `npm test`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/api/types.ts src/store/auth.ts src/store/auth.test.ts
git commit -m "feat: shared Zulip types and credential storage"
```

---

### Task 3: Server config loader + servers.json

**Files:**
- Create: `src/api/servers.ts`, `public/servers.json`
- Test: `src/api/servers.test.ts`

**Interfaces:**
- Consumes: `ServerConfig` from Task 2.
- Produces: `loadServers(fetchFn?: typeof fetch): Promise<ServerConfig[]>` — throws on HTTP error or empty/invalid list; fills `sendTopic: ''` default.

- [ ] **Step 1: Write the failing test**

`src/api/servers.test.ts`:

```ts
import { loadServers } from './servers'

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

test('loads and normalizes servers.json', async () => {
  const fetchFn = vi.fn(async () =>
    jsonResponse([{ name: 'QEC Harness', prefix: '/zulip/qec' }]),
  )
  const servers = await loadServers(fetchFn as unknown as typeof fetch)
  expect(fetchFn).toHaveBeenCalledWith('/servers.json')
  expect(servers).toEqual([{ name: 'QEC Harness', prefix: '/zulip/qec', sendTopic: '' }])
})

test('throws on HTTP error', async () => {
  const fetchFn = vi.fn(async () => jsonResponse({}, 500))
  await expect(loadServers(fetchFn as unknown as typeof fetch)).rejects.toThrow('500')
})

test('throws on empty list', async () => {
  const fetchFn = vi.fn(async () => jsonResponse([]))
  await expect(loadServers(fetchFn as unknown as typeof fetch)).rejects.toThrow('empty')
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npm test`
Expected: FAIL — cannot resolve `./servers`.

- [ ] **Step 3: Write the implementation**

`src/api/servers.ts`:

```ts
import type { ServerConfig } from './types'

export async function loadServers(fetchFn: typeof fetch = fetch): Promise<ServerConfig[]> {
  const res = await fetchFn('/servers.json')
  if (!res.ok) throw new Error(`servers.json: HTTP ${res.status}`)
  const list = (await res.json()) as ServerConfig[]
  if (!Array.isArray(list) || list.length === 0) {
    throw new Error('servers.json: empty or invalid')
  }
  return list.map((s) => ({ sendTopic: '', ...s }))
}
```

`public/servers.json`:

```json
[
  { "name": "QEC Harness", "prefix": "/zulip/qec", "sendTopic": "" }
]
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/api/servers.ts src/api/servers.test.ts public/servers.json
git commit -m "feat: server allowlist config loader"
```

---

### Task 4: ZulipClient — errors, auth header, fetchApiKey

**Files:**
- Create: `src/api/client.ts`
- Test: `src/api/client.test.ts`

**Interfaces:**
- Consumes: `Credentials` from Task 2.
- Produces:
  - `class ZulipApiError extends Error { httpStatus: number; code?: string }`
  - `class ZulipClient { constructor(creds: Credentials, fetchFn?: typeof fetch) }`
  - `ZulipClient.fetchApiKey(prefix: string, email: string, password: string, fetchFn?: typeof fetch): Promise<string>`
  - private `request(path, init?)` helper reused by Tasks 5–6 (basic-auth header, JSON parse, error mapping).

- [ ] **Step 1: Write the failing test**

`src/api/client.test.ts`:

```ts
import { ZulipClient, ZulipApiError } from './client'

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('fetchApiKey', () => {
  test('posts form credentials and returns the api key', async () => {
    const fetchFn = vi.fn(async () =>
      jsonResponse({ result: 'success', api_key: 'k123', email: 'a@b.c' }),
    )
    const key = await ZulipClient.fetchApiKey('/zulip/qec', 'a@b.c', 'pw', fetchFn as unknown as typeof fetch)
    expect(key).toBe('k123')
    const [url, init] = fetchFn.mock.calls[0] as [string, RequestInit]
    expect(url).toBe('/zulip/qec/api/v1/fetch_api_key')
    expect(init.method).toBe('POST')
    expect(String(init.body)).toBe('username=a%40b.c&password=pw')
  })

  test('maps Zulip error payloads to ZulipApiError', async () => {
    const fetchFn = vi.fn(async () =>
      jsonResponse({ result: 'error', msg: 'Your username or password is incorrect', code: 'AUTHENTICATION_FAILED' }, 403),
    )
    const err = await ZulipClient.fetchApiKey('/zulip/qec', 'a@b.c', 'bad', fetchFn as unknown as typeof fetch).catch((e) => e)
    expect(err).toBeInstanceOf(ZulipApiError)
    expect(err.code).toBe('AUTHENTICATION_FAILED')
    expect(err.httpStatus).toBe(403)
  })

  test('maps non-JSON HTTP failures to ZulipApiError', async () => {
    const fetchFn = vi.fn(async () => new Response('Bad Gateway', { status: 502 }))
    const err = await ZulipClient.fetchApiKey('/zulip/qec', 'a@b.c', 'pw', fetchFn as unknown as typeof fetch).catch((e) => e)
    expect(err).toBeInstanceOf(ZulipApiError)
    expect(err.httpStatus).toBe(502)
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npm test`
Expected: FAIL — cannot resolve `./client`.

- [ ] **Step 3: Write the implementation**

`src/api/client.ts`:

```ts
import type { Credentials } from './types'

export class ZulipApiError extends Error {
  constructor(
    message: string,
    readonly httpStatus: number,
    readonly code?: string,
  ) {
    super(message)
    this.name = 'ZulipApiError'
  }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
async function parseOrThrow(res: Response): Promise<any> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let body: any = null
  try {
    body = await res.json()
  } catch {
    // non-JSON body (proxy error page etc.)
  }
  if (!res.ok || body?.result === 'error') {
    throw new ZulipApiError(body?.msg ?? `HTTP ${res.status}`, res.status, body?.code)
  }
  return body
}

const FORM = { 'Content-Type': 'application/x-www-form-urlencoded' }

export class ZulipClient {
  constructor(
    private creds: Credentials,
    private fetchFn: typeof fetch = fetch,
  ) {}

  static async fetchApiKey(
    prefix: string,
    email: string,
    password: string,
    fetchFn: typeof fetch = fetch,
  ): Promise<string> {
    const res = await fetchFn(`${prefix}/api/v1/fetch_api_key`, {
      method: 'POST',
      headers: FORM,
      body: new URLSearchParams({ username: email, password }),
    })
    const body = await parseOrThrow(res)
    return body.api_key as string
  }

  private authHeader(): string {
    return 'Basic ' + btoa(`${this.creds.email}:${this.creds.apiKey}`)
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  protected async request(path: string, init: RequestInit = {}): Promise<any> {
    const res = await this.fetchFn(`${this.creds.prefix}/api/v1${path}`, {
      ...init,
      headers: { Authorization: this.authHeader(), ...(init.headers ?? {}) },
    })
    return parseOrThrow(res)
  }
}

export { FORM }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/api/client.ts src/api/client.test.ts
git commit -m "feat: ZulipClient auth + fetchApiKey with typed errors"
```

---

### Task 5: ZulipClient — messages (fetch, send, mark read)

**Files:**
- Modify: `src/api/client.ts` (add three methods to `ZulipClient`)
- Test: `src/api/client.messages.test.ts`

**Interfaces:**
- Consumes: `request()` helper from Task 4; `ZulipMessage`, `Credentials` from Task 2.
- Produces:
  - `getMessages(streamName: string, anchor: number | 'newest', numBefore?: number): Promise<ZulipMessage[]>` (default `numBefore = 50`)
  - `sendMessage(streamName: string, content: string): Promise<number>` — returns new message id; topic is `creds.sendTopic`
  - `markStreamRead(streamId: number): Promise<void>`

- [ ] **Step 1: Write the failing test**

`src/api/client.messages.test.ts`:

```ts
import { ZulipClient } from './client'
import type { Credentials } from './types'

const creds: Credentials = { prefix: '/zulip/qec', email: 'a@b.c', apiKey: 'k', sendTopic: '' }

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

const msg = {
  id: 7, sender_full_name: 'Agent', sender_email: 'bot@b.c',
  timestamp: 1755100000, content: '<p>hi</p>', stream_id: 1, subject: '',
}

test('getMessages narrows by stream and returns messages', async () => {
  const fetchFn = vi.fn(async () => jsonResponse({ result: 'success', messages: [msg] }))
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  const out = await client.getMessages('qec', 'newest')
  expect(out).toEqual([msg])
  const url = new URL(String(fetchFn.mock.calls[0][0]), 'http://x')
  expect(url.pathname).toBe('/zulip/qec/api/v1/messages')
  expect(url.searchParams.get('anchor')).toBe('newest')
  expect(url.searchParams.get('num_before')).toBe('50')
  expect(url.searchParams.get('num_after')).toBe('0')
  expect(JSON.parse(url.searchParams.get('narrow')!)).toEqual([{ operator: 'stream', operand: 'qec' }])
  const init = fetchFn.mock.calls[0][1] as RequestInit
  expect((init.headers as Record<string, string>).Authorization).toBe('Basic ' + btoa('a@b.c:k'))
})

test('sendMessage posts to the configured sendTopic and returns id', async () => {
  const fetchFn = vi.fn(async () => jsonResponse({ result: 'success', id: 42 }))
  const client = new ZulipClient({ ...creds, sendTopic: 'chat' }, fetchFn as unknown as typeof fetch)
  const id = await client.sendMessage('qec', 'run the scan')
  expect(id).toBe(42)
  const init = fetchFn.mock.calls[0][1] as RequestInit
  const body = new URLSearchParams(String(init.body))
  expect(body.get('type')).toBe('stream')
  expect(body.get('to')).toBe('qec')
  expect(body.get('topic')).toBe('chat')
  expect(body.get('content')).toBe('run the scan')
})

test('markStreamRead posts the stream id', async () => {
  const fetchFn = vi.fn(async () => jsonResponse({ result: 'success' }))
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  await client.markStreamRead(1)
  expect(String(fetchFn.mock.calls[0][0])).toBe('/zulip/qec/api/v1/mark_stream_as_read')
  expect(new URLSearchParams(String((fetchFn.mock.calls[0][1] as RequestInit).body)).get('stream_id')).toBe('1')
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npm test`
Expected: FAIL — `getMessages is not a function`.

- [ ] **Step 3: Add the methods**

Append inside `class ZulipClient` in `src/api/client.ts` (import `ZulipMessage` in the types import):

```ts
  async getMessages(
    streamName: string,
    anchor: number | 'newest',
    numBefore = 50,
  ): Promise<ZulipMessage[]> {
    const params = new URLSearchParams({
      anchor: String(anchor),
      num_before: String(numBefore),
      num_after: '0',
      narrow: JSON.stringify([{ operator: 'stream', operand: streamName }]),
      apply_markdown: 'true',
    })
    const body = await this.request(`/messages?${params}`)
    return body.messages as ZulipMessage[]
  }

  async sendMessage(streamName: string, content: string): Promise<number> {
    const body = await this.request('/messages', {
      method: 'POST',
      headers: FORM,
      body: new URLSearchParams({
        type: 'stream',
        to: streamName,
        topic: this.creds.sendTopic,
        content,
      }),
    })
    return body.id as number
  }

  async markStreamRead(streamId: number): Promise<void> {
    await this.request('/mark_stream_as_read', {
      method: 'POST',
      headers: FORM,
      body: new URLSearchParams({ stream_id: String(streamId) }),
    })
  }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/api/client.ts src/api/client.messages.test.ts
git commit -m "feat: ZulipClient message fetch/send/mark-read"
```

---

### Task 6: ZulipClient — event queue lifecycle

**Files:**
- Modify: `src/api/client.ts` (add `register`, `pollEvents`)
- Test: `src/api/client.events.test.ts`

**Interfaces:**
- Consumes: `request()`, `InitialState`, `ZulipEvent` from earlier tasks.
- Produces:
  - `register(): Promise<InitialState>`
  - `pollEvents(queueId: string, lastEventId: number, signal?: AbortSignal): Promise<ZulipEvent[]>` — throws `ZulipApiError` with `code === 'BAD_EVENT_QUEUE_ID'` when the queue expired (Task 13 relies on this exact code).

- [ ] **Step 1: Write the failing test**

`src/api/client.events.test.ts`:

```ts
import { ZulipClient, ZulipApiError } from './client'
import type { Credentials } from './types'

const creds: Credentials = { prefix: '/zulip/qec', email: 'a@b.c', apiKey: 'k', sendTopic: '' }

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

test('register returns normalized initial state', async () => {
  const fetchFn = vi.fn(async () =>
    jsonResponse({
      result: 'success',
      queue_id: 'q9',
      last_event_id: 5,
      subscriptions: [{ stream_id: 1, name: 'qec', description: 'QEC project', color: '#fff' }],
      unread_msgs: { streams: [{ stream_id: 1, topic: '', unread_message_ids: [7, 8] }] },
    }),
  )
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  const init = await client.register()
  expect(init).toEqual({
    queueId: 'q9',
    lastEventId: 5,
    subscriptions: [{ stream_id: 1, name: 'qec', description: 'QEC project' }],
    unread: [{ stream_id: 1, topic: '', unread_message_ids: [7, 8] }],
  })
  const body = new URLSearchParams(String((fetchFn.mock.calls[0][1] as RequestInit).body))
  expect(JSON.parse(body.get('event_types')!)).toEqual(['message', 'subscription', 'update_message_flags'])
  expect(body.get('apply_markdown')).toBe('true')
})

test('pollEvents returns events and passes queue params', async () => {
  const events = [{ id: 6, type: 'heartbeat' }]
  const fetchFn = vi.fn(async () => jsonResponse({ result: 'success', events }))
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  const out = await client.pollEvents('q9', 5)
  expect(out).toEqual(events)
  const url = new URL(String(fetchFn.mock.calls[0][0]), 'http://x')
  expect(url.pathname).toBe('/zulip/qec/api/v1/events')
  expect(url.searchParams.get('queue_id')).toBe('q9')
  expect(url.searchParams.get('last_event_id')).toBe('5')
})

test('pollEvents surfaces BAD_EVENT_QUEUE_ID as a typed error', async () => {
  const fetchFn = vi.fn(async () =>
    jsonResponse({ result: 'error', msg: 'Bad event queue ID', code: 'BAD_EVENT_QUEUE_ID' }, 400),
  )
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  const err = await client.pollEvents('dead', 5).catch((e) => e)
  expect(err).toBeInstanceOf(ZulipApiError)
  expect(err.code).toBe('BAD_EVENT_QUEUE_ID')
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npm test`
Expected: FAIL — `register is not a function`.

- [ ] **Step 3: Add the methods**

Append inside `class ZulipClient` (extend the types import with `InitialState, StreamSub, ZulipEvent`):

```ts
  async register(): Promise<InitialState> {
    const body = await this.request('/register', {
      method: 'POST',
      headers: FORM,
      body: new URLSearchParams({
        event_types: JSON.stringify(['message', 'subscription', 'update_message_flags']),
        apply_markdown: 'true',
        client_gravatar: 'true',
      }),
    })
    return {
      queueId: body.queue_id as string,
      lastEventId: body.last_event_id as number,
      subscriptions: ((body.subscriptions ?? []) as StreamSub[]).map((s) => ({
        stream_id: s.stream_id,
        name: s.name,
        description: s.description,
      })),
      unread: body.unread_msgs?.streams ?? [],
    }
  }

  async pollEvents(
    queueId: string,
    lastEventId: number,
    signal?: AbortSignal,
  ): Promise<ZulipEvent[]> {
    const params = new URLSearchParams({
      queue_id: queueId,
      last_event_id: String(lastEventId),
    })
    const res = await this.fetchFn(`${this.creds.prefix}/api/v1/events?${params}`, {
      headers: { Authorization: this.authHeader() },
      signal,
    })
    const body = await parseOrThrow(res)
    return body.events as ZulipEvent[]
  }
```

Note: `pollEvents` bypasses `request()` only because it needs the `signal` option; keep the auth-header logic identical.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/api/client.ts src/api/client.events.test.ts
git commit -m "feat: ZulipClient event queue register/poll"
```

---

### Task 7: App store (zustand)

**Files:**
- Create: `src/store/appStore.ts`
- Test: `src/store/appStore.test.ts`

**Interfaces:**
- Consumes: types from Task 2; `ZulipClient` from Task 4; `clearCredentials` from Task 2.
- Produces (exact shape — all views and hooks rely on it):

```ts
type View = { name: 'projects' } | { name: 'conversation'; streamId: number }
type Connection = 'live' | 'connecting' | 'offline'
interface AppState {
  creds: Credentials | null
  client: ZulipClient | null
  view: View
  settingsOpen: boolean
  streams: StreamSub[]
  unreadByStream: Record<number, number[]>
  messagesByStream: Record<number, ZulipMessage[]>
  hiddenStreams: number[]
  connection: Connection
  setCreds(c: Credentials): void          // also constructs client, navigates to projects
  logout(): void                          // clears storage + all data, drops client
  navigate(v: View): void
  setSettingsOpen(open: boolean): void
  applyInitialState(s: InitialState): void
  setMessages(streamId: number, msgs: ZulipMessage[]): void
  prependOlder(streamId: number, msgs: ZulipMessage[]): void
  applyEvents(events: ZulipEvent[]): void
  clearUnread(streamId: number): void
  toggleHidden(streamId: number): void    // persists to localStorage 'zulip-app.hidden'
  setConnection(c: Connection): void
}
export const useAppStore: UseBoundStore<StoreApi<AppState>>
export function resetAppStore(): void     // test helper: restores initial data fields
```

- [ ] **Step 1: Write the failing test**

`src/store/appStore.test.ts`:

```ts
import { useAppStore, resetAppStore } from './appStore'
import { loadCredentials } from './auth'
import type { Credentials, InitialState, ZulipMessage } from '../api/types'

const creds: Credentials = { prefix: '/zulip/qec', email: 'me@b.c', apiKey: 'k', sendTopic: '' }

const initial: InitialState = {
  queueId: 'q1',
  lastEventId: 0,
  subscriptions: [
    { stream_id: 2, name: 'beta', description: 'B' },
    { stream_id: 1, name: 'alpha', description: 'A' },
  ],
  unread: [
    { stream_id: 1, topic: '', unread_message_ids: [10] },
    { stream_id: 1, topic: 'chat', unread_message_ids: [11] },
  ],
}

function makeMsg(id: number, sender = 'bot@b.c'): ZulipMessage {
  return {
    id, sender_full_name: 'Bot', sender_email: sender,
    timestamp: 1755100000 + id, content: `<p>m${id}</p>`, stream_id: 1, subject: '',
  }
}

beforeEach(() => resetAppStore())

test('setCreds stores creds, builds a client, navigates to projects', () => {
  useAppStore.getState().setCreds(creds)
  const s = useAppStore.getState()
  expect(s.creds).toEqual(creds)
  expect(s.client).not.toBeNull()
  expect(s.view).toEqual({ name: 'projects' })
})

test('applyInitialState sorts streams and merges per-topic unreads', () => {
  useAppStore.getState().applyInitialState(initial)
  const s = useAppStore.getState()
  expect(s.streams.map((x) => x.name)).toEqual(['alpha', 'beta'])
  expect(s.unreadByStream[1]).toEqual([10, 11])
})

test('message events append to loaded streams and count unread for others only', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().setMessages(1, [makeMsg(1)])
  useAppStore.getState().applyEvents([
    { id: 1, type: 'message', message: makeMsg(2) },
    { id: 2, type: 'message', message: makeMsg(3, 'me@b.c') },
  ])
  const s = useAppStore.getState()
  expect(s.messagesByStream[1].map((m) => m.id)).toEqual([1, 2, 3])
  expect(s.unreadByStream[1]).toEqual([2]) // own message never unread
})

test('read-flag events remove unreads', () => {
  useAppStore.getState().applyInitialState(initial)
  useAppStore.getState().applyEvents([
    { id: 3, type: 'update_message_flags', flag: 'read', op: 'add', messages: [10] },
  ])
  expect(useAppStore.getState().unreadByStream[1]).toEqual([11])
})

test('toggleHidden persists to localStorage and survives resetAppStore-like reload', () => {
  useAppStore.getState().toggleHidden(1)
  expect(useAppStore.getState().hiddenStreams).toEqual([1])
  expect(JSON.parse(localStorage.getItem('zulip-app.hidden')!)).toEqual([1])
  useAppStore.getState().toggleHidden(1)
  expect(useAppStore.getState().hiddenStreams).toEqual([])
})

test('prependOlder puts messages before existing ones', () => {
  useAppStore.getState().setMessages(1, [makeMsg(5)])
  useAppStore.getState().prependOlder(1, [makeMsg(3), makeMsg(4)])
  expect(useAppStore.getState().messagesByStream[1].map((m) => m.id)).toEqual([3, 4, 5])
})

test('logout clears everything including stored credentials', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().applyInitialState(initial)
  useAppStore.getState().logout()
  const s = useAppStore.getState()
  expect(s.creds).toBeNull()
  expect(s.client).toBeNull()
  expect(s.streams).toEqual([])
  expect(loadCredentials()).toBeNull()
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npm test`
Expected: FAIL — cannot resolve `./appStore`.

- [ ] **Step 3: Write the implementation**

`src/store/appStore.ts`:

```ts
import { create } from 'zustand'
import { ZulipClient } from '../api/client'
import {
  isMessageEvent,
  isReadFlagsEvent,
  type Credentials,
  type InitialState,
  type StreamSub,
  type ZulipEvent,
  type ZulipMessage,
} from '../api/types'
import { saveCredentials, clearCredentials } from './auth'

const HIDDEN_KEY = 'zulip-app.hidden'

export type View = { name: 'projects' } | { name: 'conversation'; streamId: number }
export type Connection = 'live' | 'connecting' | 'offline'

export interface AppState {
  creds: Credentials | null
  client: ZulipClient | null
  view: View
  settingsOpen: boolean
  streams: StreamSub[]
  unreadByStream: Record<number, number[]>
  messagesByStream: Record<number, ZulipMessage[]>
  hiddenStreams: number[]
  connection: Connection
  setCreds(c: Credentials): void
  logout(): void
  navigate(v: View): void
  setSettingsOpen(open: boolean): void
  applyInitialState(s: InitialState): void
  setMessages(streamId: number, msgs: ZulipMessage[]): void
  prependOlder(streamId: number, msgs: ZulipMessage[]): void
  applyEvents(events: ZulipEvent[]): void
  clearUnread(streamId: number): void
  toggleHidden(streamId: number): void
  setConnection(c: Connection): void
}

function loadHidden(): number[] {
  try {
    const raw = localStorage.getItem(HIDDEN_KEY)
    return raw ? (JSON.parse(raw) as number[]) : []
  } catch {
    return []
  }
}

const initialData = {
  creds: null as Credentials | null,
  client: null as ZulipClient | null,
  view: { name: 'projects' } as View,
  settingsOpen: false,
  streams: [] as StreamSub[],
  unreadByStream: {} as Record<number, number[]>,
  messagesByStream: {} as Record<number, ZulipMessage[]>,
  hiddenStreams: [] as number[],
  connection: 'connecting' as Connection,
}

export const useAppStore = create<AppState>()((set, get) => ({
  ...initialData,
  hiddenStreams: loadHidden(),

  setCreds: (c) => {
    saveCredentials(c)
    set({ creds: c, client: new ZulipClient(c), view: { name: 'projects' } })
  },

  logout: () => {
    clearCredentials()
    set({ ...initialData, hiddenStreams: get().hiddenStreams })
  },

  navigate: (v) => set({ view: v }),
  setSettingsOpen: (open) => set({ settingsOpen: open }),

  applyInitialState: (s) => {
    const unreadByStream: Record<number, number[]> = {}
    for (const entry of s.unread) {
      unreadByStream[entry.stream_id] = [
        ...(unreadByStream[entry.stream_id] ?? []),
        ...entry.unread_message_ids,
      ]
    }
    set({
      streams: [...s.subscriptions].sort((a, b) => a.name.localeCompare(b.name)),
      unreadByStream,
    })
  },

  setMessages: (streamId, msgs) =>
    set((state) => ({ messagesByStream: { ...state.messagesByStream, [streamId]: msgs } })),

  prependOlder: (streamId, msgs) =>
    set((state) => ({
      messagesByStream: {
        ...state.messagesByStream,
        [streamId]: [...msgs, ...(state.messagesByStream[streamId] ?? [])],
      },
    })),

  applyEvents: (events) =>
    set((state) => {
      const messagesByStream = { ...state.messagesByStream }
      const unreadByStream = { ...state.unreadByStream }
      const self = state.creds?.email
      for (const ev of events) {
        if (isMessageEvent(ev)) {
          const m = ev.message
          const list = messagesByStream[m.stream_id]
          if (list && !list.some((x) => x.id === m.id)) {
            messagesByStream[m.stream_id] = [...list, m]
          }
          if (m.sender_email !== self) {
            unreadByStream[m.stream_id] = [...(unreadByStream[m.stream_id] ?? []), m.id]
          }
        } else if (isReadFlagsEvent(ev) && (ev.op ?? ev.operation) === 'add') {
          const read = new Set(ev.messages)
          for (const key of Object.keys(unreadByStream)) {
            const sid = Number(key)
            unreadByStream[sid] = unreadByStream[sid].filter((id) => !read.has(id))
          }
        }
        // 'subscription' and unknown event types are intentionally ignored in v1;
        // stream list refreshes on the next register().
      }
      return { messagesByStream, unreadByStream }
    }),

  clearUnread: (streamId) =>
    set((state) => ({ unreadByStream: { ...state.unreadByStream, [streamId]: [] } })),

  toggleHidden: (streamId) =>
    set((state) => {
      const hiddenStreams = state.hiddenStreams.includes(streamId)
        ? state.hiddenStreams.filter((id) => id !== streamId)
        : [...state.hiddenStreams, streamId]
      localStorage.setItem(HIDDEN_KEY, JSON.stringify(hiddenStreams))
      return { hiddenStreams }
    }),

  setConnection: (c) => set({ connection: c }),
}))

export function resetAppStore(): void {
  useAppStore.setState({ ...initialData, hiddenStreams: [] })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/store/appStore.ts src/store/appStore.test.ts
git commit -m "feat: zustand app store with event application and prefs"
```

---

### Task 8: MessageBody — sanitize, rewrite, render

**Files:**
- Create: `src/components/MessageBody.tsx`, `src/test/fixtures/zulipHtml.ts`
- Test: `src/components/MessageBody.test.tsx`

**Interfaces:**
- Consumes: nothing app-specific (pure function + presentational component).
- Produces:
  - `sanitizeZulipHtml(html: string, prefix: string): string` (exported for tests)
  - `MessageBody({ html, prefix }: { html: string; prefix: string })` — renders sanitized HTML in `div.message-body`.

- [ ] **Step 1: Write fixtures**

`src/test/fixtures/zulipHtml.ts` (patterned on real Zulip renderer output; replace with server-captured strings when first connecting to the real realm):

```ts
export const codeBlock =
  '<div class="codehilite" data-code-language="Python"><pre><span></span><code><span class="k">def</span> <span class="nf">f</span><span class="p">():</span>\n    <span class="k">pass</span>\n</code></pre></div>'

export const katexMath =
  '<p><span class="katex"><span class="katex-html" aria-hidden="true"><span class="base"><span class="mord mathnormal">x</span></span></span></span></p>'

export const uploadLink =
  '<p><a href="/user_uploads/2/ab/report.pdf">report.pdf</a></p>'

export const inlineImage =
  '<div class="message_inline_image"><a href="/user_uploads/2/ab/plot.png"><img src="/user_uploads/2/ab/plot.png"></a></div>'

export const externalLink =
  '<p><a href="https://arxiv.org/abs/2401.00001">paper</a></p>'

export const hostileScript = '<p>hi</p><script>window.hacked = true</script>'

export const hostileImgHandler = '<img src="x" onerror="window.hacked = true">'

export const hostileJsHref = '<a href="javascript:window.hacked=1">click</a>'
```

- [ ] **Step 2: Write the failing test**

`src/components/MessageBody.test.tsx`:

```tsx
import { render } from '@testing-library/react'
import { MessageBody, sanitizeZulipHtml } from './MessageBody'
import * as fx from '../test/fixtures/zulipHtml'

const PREFIX = '/zulip/qec'

test('keeps code block structure and classes', () => {
  const out = sanitizeZulipHtml(fx.codeBlock, PREFIX)
  expect(out).toContain('codehilite')
  expect(out).toContain('<pre>')
  expect(out).toContain('class="k"')
})

test('keeps KaTeX spans', () => {
  expect(sanitizeZulipHtml(fx.katexMath, PREFIX)).toContain('katex')
})

test('rewrites relative upload links and images to the proxy prefix', () => {
  expect(sanitizeZulipHtml(fx.uploadLink, PREFIX)).toContain('href="/zulip/qec/user_uploads/2/ab/report.pdf"')
  expect(sanitizeZulipHtml(fx.inlineImage, PREFIX)).toContain('src="/zulip/qec/user_uploads/2/ab/plot.png"')
})

test('leaves absolute external links alone but adds rel/target', () => {
  const out = sanitizeZulipHtml(fx.externalLink, PREFIX)
  expect(out).toContain('href="https://arxiv.org/abs/2401.00001"')
  expect(out).toContain('target="_blank"')
  expect(out).toContain('rel="noopener noreferrer"')
})

test.each([
  ['script tag', fx.hostileScript],
  ['img onerror', fx.hostileImgHandler],
  ['javascript: href', fx.hostileJsHref],
])('strips hostile payload: %s', (_name, html) => {
  const out = sanitizeZulipHtml(html, PREFIX)
  expect(out).not.toContain('script')
  expect(out).not.toContain('onerror')
  expect(out).not.toContain('javascript:')
})

test('component renders sanitized HTML', () => {
  const { container } = render(<MessageBody html={fx.codeBlock} prefix={PREFIX} />)
  expect(container.querySelector('.message-body pre')).not.toBeNull()
})
```

- [ ] **Step 3: Run test to verify it fails**

Run: `npm test`
Expected: FAIL — cannot resolve `./MessageBody`.

- [ ] **Step 4: Write the implementation**

`src/components/MessageBody.tsx`:

```tsx
import DOMPurify from 'dompurify'

const ALLOWED_TAGS = [
  'a', 'p', 'br', 'span', 'div', 'strong', 'em', 'del', 'code', 'pre',
  'blockquote', 'ul', 'ol', 'li', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
  'table', 'thead', 'tbody', 'tr', 'th', 'td', 'img', 'hr', 'sup', 'sub',
  'time', 'details', 'summary',
]
const ALLOWED_ATTR = ['href', 'src', 'alt', 'title', 'class', 'start', 'datetime', 'aria-hidden', 'data-code-language']

export function sanitizeZulipHtml(html: string, prefix: string): string {
  const clean = DOMPurify.sanitize(html, { ALLOWED_TAGS, ALLOWED_ATTR })
  const doc = new DOMParser().parseFromString(clean, 'text/html')
  for (const a of Array.from(doc.querySelectorAll('a'))) {
    const href = a.getAttribute('href') ?? ''
    if (href.startsWith('/')) a.setAttribute('href', prefix + href)
    a.setAttribute('target', '_blank')
    a.setAttribute('rel', 'noopener noreferrer')
  }
  for (const img of Array.from(doc.querySelectorAll('img'))) {
    const src = img.getAttribute('src') ?? ''
    if (src.startsWith('/')) img.setAttribute('src', prefix + src)
  }
  return doc.body.innerHTML
}

export function MessageBody({ html, prefix }: { html: string; prefix: string }) {
  return (
    <div
      className="message-body"
      dangerouslySetInnerHTML={{ __html: sanitizeZulipHtml(html, prefix) }}
    />
  )
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `npm test`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/components/MessageBody.tsx src/components/MessageBody.test.tsx src/test/fixtures/zulipHtml.ts
git commit -m "feat: sanitized Zulip HTML renderer with proxy URL rewriting"
```

---

### Task 9: LoginView

**Files:**
- Create: `src/views/LoginView.tsx`
- Test: `src/views/LoginView.test.tsx`

**Interfaces:**
- Consumes: `loadServers` (Task 3), `ZulipClient.fetchApiKey` (Task 4), `useAppStore.setCreds` (Task 7), `Credentials`/`ServerConfig` (Task 2).
- Produces: `LoginView()` component. Server picker renders only when `servers.length > 1`.

- [ ] **Step 1: Write the failing test**

`src/views/LoginView.test.tsx`:

```tsx
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { LoginView } from './LoginView'
import { ZulipClient } from '../api/client'
import { useAppStore, resetAppStore } from '../store/appStore'

vi.mock('../api/servers', () => ({
  loadServers: vi.fn(async () => [
    { name: 'QEC Harness', prefix: '/zulip/qec', sendTopic: '' },
  ]),
}))

beforeEach(() => resetAppStore())

test('password login fetches api key and stores credentials', async () => {
  const spy = vi.spyOn(ZulipClient, 'fetchApiKey').mockResolvedValue('key99')
  render(<LoginView />)
  await userEvent.type(await screen.findByLabelText(/email/i), 'a@b.c')
  await userEvent.type(screen.getByLabelText(/password/i), 'pw')
  await userEvent.click(screen.getByRole('button', { name: /sign in/i }))
  await waitFor(() => expect(useAppStore.getState().creds).not.toBeNull())
  expect(spy).toHaveBeenCalledWith('/zulip/qec', 'a@b.c', 'pw')
  expect(useAppStore.getState().creds).toEqual({
    prefix: '/zulip/qec', email: 'a@b.c', apiKey: 'key99', sendTopic: '',
  })
})

test('paste-API-key path skips fetchApiKey', async () => {
  const spy = vi.spyOn(ZulipClient, 'fetchApiKey')
  render(<LoginView />)
  await userEvent.click(await screen.findByRole('button', { name: /paste api key/i }))
  await userEvent.type(screen.getByLabelText(/email/i), 'a@b.c')
  await userEvent.type(screen.getByLabelText(/api key/i), 'direct-key')
  await userEvent.click(screen.getByRole('button', { name: /sign in/i }))
  await waitFor(() => expect(useAppStore.getState().creds?.apiKey).toBe('direct-key'))
  expect(spy).not.toHaveBeenCalled()
})

test('shows auth errors and keeps the form', async () => {
  vi.spyOn(ZulipClient, 'fetchApiKey').mockRejectedValue(new Error('Your username or password is incorrect'))
  render(<LoginView />)
  await userEvent.type(await screen.findByLabelText(/email/i), 'a@b.c')
  await userEvent.type(screen.getByLabelText(/password/i), 'bad')
  await userEvent.click(screen.getByRole('button', { name: /sign in/i }))
  expect(await screen.findByRole('alert')).toHaveTextContent(/incorrect/)
  expect(useAppStore.getState().creds).toBeNull()
})

test('single server: no server picker rendered', async () => {
  render(<LoginView />)
  await screen.findByLabelText(/email/i)
  expect(screen.queryByLabelText(/server/i)).toBeNull()
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npm test`
Expected: FAIL — cannot resolve `./LoginView`.

- [ ] **Step 3: Write the implementation**

`src/views/LoginView.tsx`:

```tsx
import { useEffect, useState, type FormEvent } from 'react'
import { loadServers } from '../api/servers'
import { ZulipClient } from '../api/client'
import { useAppStore } from '../store/appStore'
import type { Credentials, ServerConfig } from '../api/types'

export function LoginView() {
  const [servers, setServers] = useState<ServerConfig[] | null>(null)
  const [serverIdx, setServerIdx] = useState(0)
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [useKey, setUseKey] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const setCreds = useAppStore((s) => s.setCreds)

  useEffect(() => {
    loadServers().then(setServers).catch((e) => setError(e instanceof Error ? e.message : String(e)))
  }, [])

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!servers) return
    setBusy(true)
    setError(null)
    const server = servers[serverIdx]
    try {
      const key = useKey ? apiKey : await ZulipClient.fetchApiKey(server.prefix, email, password)
      const creds: Credentials = {
        prefix: server.prefix,
        email,
        apiKey: key,
        sendTopic: server.sendTopic ?? '',
      }
      setCreds(creds)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  if (!servers) return <p className="empty">{error ?? 'Loading…'}</p>

  return (
    <form className="login" onSubmit={submit}>
      <h1>Agent Console</h1>
      {servers.length > 1 && (
        <label>
          Server
          <select value={serverIdx} onChange={(e) => setServerIdx(Number(e.target.value))}>
            {servers.map((s, i) => (
              <option key={s.prefix} value={i}>{s.name}</option>
            ))}
          </select>
        </label>
      )}
      <label>
        Email
        <input type="email" required value={email} onChange={(e) => setEmail(e.target.value)} />
      </label>
      {useKey ? (
        <label>
          API key
          <input type="password" required value={apiKey} onChange={(e) => setApiKey(e.target.value)} />
        </label>
      ) : (
        <label>
          Password
          <input type="password" required value={password} onChange={(e) => setPassword(e.target.value)} />
        </label>
      )}
      {error && <p role="alert" className="error">{error}</p>}
      <button type="submit" disabled={busy}>{busy ? 'Signing in…' : 'Sign in'}</button>
      <button type="button" className="link" onClick={() => setUseKey(!useKey)}>
        {useKey ? 'Use password instead' : 'Paste API key instead'}
      </button>
    </form>
  )
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/views/LoginView.tsx src/views/LoginView.test.tsx
git commit -m "feat: login view with password and paste-key paths"
```

---

### Task 10: ProjectsView

**Files:**
- Create: `src/views/ProjectsView.tsx`
- Test: `src/views/ProjectsView.test.tsx`

**Interfaces:**
- Consumes: store fields `streams`, `hiddenStreams`, `unreadByStream`, actions `navigate`, `setSettingsOpen` (Task 7).
- Produces: `ProjectsView()` — stream cards filtered by hidden prefs; tapping a card navigates to `{ name: 'conversation', streamId }`; gear button opens settings.

- [ ] **Step 1: Write the failing test**

`src/views/ProjectsView.test.tsx`:

```tsx
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ProjectsView } from './ProjectsView'
import { useAppStore, resetAppStore } from '../store/appStore'

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({
    streams: [
      { stream_id: 1, name: 'alpha', description: 'Project A' },
      { stream_id: 2, name: 'beta', description: 'Project B' },
    ],
    unreadByStream: { 1: [10, 11] },
  })
})

test('renders visible streams with unread badges', () => {
  render(<ProjectsView />)
  expect(screen.getByText('alpha')).toBeInTheDocument()
  expect(screen.getByText('Project A')).toBeInTheDocument()
  expect(screen.getByText('2')).toBeInTheDocument() // unread badge
})

test('hidden streams are filtered out', () => {
  useAppStore.setState({ hiddenStreams: [2] })
  render(<ProjectsView />)
  expect(screen.queryByText('beta')).toBeNull()
})

test('tapping a card navigates to the conversation', async () => {
  render(<ProjectsView />)
  await userEvent.click(screen.getByRole('button', { name: /alpha/ }))
  expect(useAppStore.getState().view).toEqual({ name: 'conversation', streamId: 1 })
})

test('gear opens settings', async () => {
  render(<ProjectsView />)
  await userEvent.click(screen.getByRole('button', { name: /settings/i }))
  expect(useAppStore.getState().settingsOpen).toBe(true)
})

test('empty state message when nothing visible', () => {
  useAppStore.setState({ streams: [] })
  render(<ProjectsView />)
  expect(screen.getByText(/no projects/i)).toBeInTheDocument()
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npm test`
Expected: FAIL — cannot resolve `./ProjectsView`.

- [ ] **Step 3: Write the implementation**

`src/views/ProjectsView.tsx`:

```tsx
import { useAppStore } from '../store/appStore'

export function ProjectsView() {
  const streams = useAppStore((s) => s.streams)
  const hidden = useAppStore((s) => s.hiddenStreams)
  const unread = useAppStore((s) => s.unreadByStream)
  const navigate = useAppStore((s) => s.navigate)
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen)
  const visible = streams.filter((s) => !hidden.includes(s.stream_id))

  return (
    <div className="projects">
      <header className="topbar">
        <h1>Projects</h1>
        <button aria-label="Settings" onClick={() => setSettingsOpen(true)}>⚙</button>
      </header>
      {visible.length === 0 && (
        <p className="empty">No projects. Subscribe to streams on Zulip's web UI.</p>
      )}
      <ul className="stream-list">
        {visible.map((s) => {
          const count = unread[s.stream_id]?.length ?? 0
          return (
            <li key={s.stream_id}>
              <button
                className="stream-card"
                onClick={() => navigate({ name: 'conversation', streamId: s.stream_id })}
              >
                <span className="stream-name">{s.name}</span>
                {count > 0 && <span className="unread-badge">{count}</span>}
                <span className="stream-desc">{s.description}</span>
              </button>
            </li>
          )
        })}
      </ul>
    </div>
  )
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/views/ProjectsView.tsx src/views/ProjectsView.test.tsx
git commit -m "feat: projects view with unread badges and hide filter"
```

---

### Task 11: ConversationView + Composer

**Files:**
- Create: `src/views/ConversationView.tsx`, `src/components/Composer.tsx`
- Test: `src/views/ConversationView.test.tsx`

**Interfaces:**
- Consumes: store (Task 7), `MessageBody` (Task 8), `ZulipClient.getMessages/sendMessage/markStreamRead` (Tasks 5).
- Produces: `ConversationView({ streamId }: { streamId: number })`, `Composer({ streamName }: { streamName: string })`. Back button navigates to projects.

- [ ] **Step 1: Write the failing test**

`src/views/ConversationView.test.tsx`:

```tsx
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ConversationView } from './ConversationView'
import { useAppStore, resetAppStore } from '../store/appStore'
import type { ZulipClient } from '../api/client'
import type { ZulipMessage } from '../api/types'

function makeMsg(id: number): ZulipMessage {
  return {
    id, sender_full_name: 'Agent', sender_email: 'bot@b.c',
    timestamp: 1755100000 + id, content: `<p>msg-${id}</p>`, stream_id: 1, subject: '',
  }
}

function fakeClient(overrides: Partial<Record<keyof ZulipClient, unknown>> = {}) {
  return {
    getMessages: vi.fn(async () => [makeMsg(1), makeMsg(2)]),
    sendMessage: vi.fn(async () => 99),
    markStreamRead: vi.fn(async () => {}),
    ...overrides,
  } as unknown as ZulipClient
}

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({
    creds: { prefix: '/zulip/qec', email: 'me@b.c', apiKey: 'k', sendTopic: '' },
    streams: [{ stream_id: 1, name: 'alpha', description: 'A' }],
  })
})

test('loads newest messages on mount and renders them sanitized', async () => {
  const client = fakeClient()
  useAppStore.setState({ client })
  render(<ConversationView streamId={1} />)
  expect(await screen.findByText('msg-1')).toBeInTheDocument()
  expect(client.getMessages).toHaveBeenCalledWith('alpha', 'newest')
  await waitFor(() => expect(client.markStreamRead).toHaveBeenCalledWith(1))
  expect(useAppStore.getState().unreadByStream[1] ?? []).toEqual([])
})

test('load earlier prepends older messages without duplicates', async () => {
  const client = fakeClient({
    getMessages: vi
      .fn()
      .mockResolvedValueOnce([makeMsg(10), makeMsg(11)])
      .mockResolvedValueOnce([makeMsg(9), makeMsg(10)]),
  })
  useAppStore.setState({ client })
  render(<ConversationView streamId={1} />)
  await screen.findByText('msg-10')
  await userEvent.click(screen.getByRole('button', { name: /load earlier/i }))
  await screen.findByText('msg-9')
  expect(useAppStore.getState().messagesByStream[1].map((m) => m.id)).toEqual([9, 10, 11])
})

test('send clears composer on success', async () => {
  const client = fakeClient()
  useAppStore.setState({ client })
  render(<ConversationView streamId={1} />)
  await screen.findByText('msg-1')
  const box = screen.getByRole('textbox')
  await userEvent.type(box, 'do the thing')
  await userEvent.click(screen.getByRole('button', { name: /^send$/i }))
  await waitFor(() => expect(box).toHaveValue(''))
  expect(client.sendMessage).toHaveBeenCalledWith('alpha', 'do the thing')
})

test('failed send keeps text and offers retry', async () => {
  const client = fakeClient({
    sendMessage: vi.fn().mockRejectedValueOnce(new Error('boom')).mockResolvedValueOnce(99),
  })
  useAppStore.setState({ client })
  render(<ConversationView streamId={1} />)
  await screen.findByText('msg-1')
  const box = screen.getByRole('textbox')
  await userEvent.type(box, 'important command')
  await userEvent.click(screen.getByRole('button', { name: /^send$/i }))
  expect(await screen.findByText(/send failed/i)).toBeInTheDocument()
  expect(box).toHaveValue('important command')
  await userEvent.click(screen.getByRole('button', { name: /retry/i }))
  await waitFor(() => expect(box).toHaveValue(''))
})

test('back button returns to projects', async () => {
  useAppStore.setState({ client: fakeClient(), view: { name: 'conversation', streamId: 1 } })
  render(<ConversationView streamId={1} />)
  await userEvent.click(screen.getByRole('button', { name: /back/i }))
  expect(useAppStore.getState().view).toEqual({ name: 'projects' })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npm test`
Expected: FAIL — cannot resolve `./ConversationView`.

- [ ] **Step 3: Write the implementation**

`src/components/Composer.tsx`:

```tsx
import { useState } from 'react'
import { useAppStore } from '../store/appStore'

export function Composer({ streamName }: { streamName: string }) {
  const client = useAppStore((s) => s.client)
  const [text, setText] = useState('')
  const [failed, setFailed] = useState(false)
  const [sending, setSending] = useState(false)

  async function send() {
    if (!client || !text.trim() || sending) return
    setSending(true)
    try {
      await client.sendMessage(streamName, text)
      setText('')
      setFailed(false)
    } catch {
      setFailed(true)
    } finally {
      setSending(false)
    }
  }

  return (
    <div>
      {failed && <p className="error" role="alert">Send failed — check connection and retry.</p>}
      <div className="composer">
        <textarea
          rows={2}
          value={text}
          placeholder="Message the agent…"
          onChange={(e) => setText(e.target.value)}
        />
        <button onClick={send} disabled={sending || !text.trim()}>
          {failed ? 'Retry' : 'Send'}
        </button>
      </div>
    </div>
  )
}
```

`src/views/ConversationView.tsx`:

```tsx
import { useEffect, useState } from 'react'
import { useAppStore } from '../store/appStore'
import { MessageBody } from '../components/MessageBody'
import { Composer } from '../components/Composer'

export function ConversationView({ streamId }: { streamId: number }) {
  const stream = useAppStore((s) => s.streams.find((x) => x.stream_id === streamId))
  const messages = useAppStore((s) => s.messagesByStream[streamId])
  const creds = useAppStore((s) => s.creds)
  const client = useAppStore((s) => s.client)
  const navigate = useAppStore((s) => s.navigate)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [loadingOlder, setLoadingOlder] = useState(false)
  const name = stream?.name ?? ''
  const loaded = messages !== undefined
  const messageCount = messages?.length ?? 0

  useEffect(() => {
    if (!client || !stream || loaded) return
    client
      .getMessages(name, 'newest')
      .then((msgs) => useAppStore.getState().setMessages(streamId, msgs))
      .catch((e) => setLoadError(e instanceof Error ? e.message : String(e)))
  }, [client, stream, loaded, name, streamId])

  useEffect(() => {
    if (!client || !loaded) return
    client.markStreamRead(streamId).catch(() => {})
    useAppStore.getState().clearUnread(streamId)
  }, [client, streamId, loaded, messageCount])

  async function loadOlder() {
    if (!client || !messages || messages.length === 0 || loadingOlder) return
    setLoadingOlder(true)
    try {
      const older = await client.getMessages(name, messages[0].id, 51)
      const known = new Set(messages.map((m) => m.id))
      useAppStore.getState().prependOlder(streamId, older.filter((m) => !known.has(m.id)))
    } catch {
      setLoadError('Could not load earlier messages.')
    } finally {
      setLoadingOlder(false)
    }
  }

  if (!stream || !creds) return null

  return (
    <div className="conversation">
      <header className="topbar">
        <button aria-label="Back" onClick={() => navigate({ name: 'projects' })}>‹</button>
        <h1>{stream.name}</h1>
      </header>
      <div className="message-scroll">
        {loaded && messages.length > 0 && (
          <button className="load-earlier" onClick={loadOlder} disabled={loadingOlder}>
            {loadingOlder ? 'Loading…' : 'Load earlier'}
          </button>
        )}
        {loadError && <p className="error" role="alert">{loadError}</p>}
        {!loaded && !loadError && <p className="empty">Loading…</p>}
        {messages?.map((m) => (
          <div className="message" key={m.id}>
            <div className="message-meta">
              <span className="sender">{m.sender_full_name}</span>{' '}
              {new Date(m.timestamp * 1000).toLocaleString()}
            </div>
            <MessageBody html={m.content} prefix={creds.prefix} />
          </div>
        ))}
      </div>
      <Composer streamName={name} />
    </div>
  )
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/views/ConversationView.tsx src/components/Composer.tsx src/views/ConversationView.test.tsx
git commit -m "feat: conversation view with history, mark-read, and composer"
```

---

### Task 12: SettingsSheet

**Files:**
- Create: `src/views/SettingsSheet.tsx`
- Test: `src/views/SettingsSheet.test.tsx`

**Interfaces:**
- Consumes: store fields `creds`, `streams`, `hiddenStreams`, actions `toggleHidden`, `setSettingsOpen`, `logout` (Task 7).
- Produces: `SettingsSheet()` — full-screen sheet: identity line, per-stream visibility checkboxes, Close, Log out.

- [ ] **Step 1: Write the failing test**

`src/views/SettingsSheet.test.tsx`:

```tsx
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { SettingsSheet } from './SettingsSheet'
import { useAppStore, resetAppStore } from '../store/appStore'

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({
    creds: { prefix: '/zulip/qec', email: 'me@b.c', apiKey: 'k', sendTopic: '' },
    settingsOpen: true,
    streams: [
      { stream_id: 1, name: 'alpha', description: 'A' },
      { stream_id: 2, name: 'beta', description: 'B' },
    ],
    hiddenStreams: [2],
  })
})

test('shows identity and stream checkboxes reflecting hidden state', () => {
  render(<SettingsSheet />)
  expect(screen.getByText(/me@b\.c/)).toBeInTheDocument()
  expect(screen.getByRole('checkbox', { name: 'alpha' })).toBeChecked()
  expect(screen.getByRole('checkbox', { name: 'beta' })).not.toBeChecked()
})

test('toggling a checkbox flips hidden state', async () => {
  render(<SettingsSheet />)
  await userEvent.click(screen.getByRole('checkbox', { name: 'beta' }))
  expect(useAppStore.getState().hiddenStreams).toEqual([])
})

test('close button dismisses the sheet', async () => {
  render(<SettingsSheet />)
  await userEvent.click(screen.getByRole('button', { name: /close/i }))
  expect(useAppStore.getState().settingsOpen).toBe(false)
})

test('log out clears credentials', async () => {
  render(<SettingsSheet />)
  await userEvent.click(screen.getByRole('button', { name: /log out/i }))
  expect(useAppStore.getState().creds).toBeNull()
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npm test`
Expected: FAIL — cannot resolve `./SettingsSheet`.

- [ ] **Step 3: Write the implementation**

`src/views/SettingsSheet.tsx`:

```tsx
import { useAppStore } from '../store/appStore'

export function SettingsSheet() {
  const creds = useAppStore((s) => s.creds)
  const streams = useAppStore((s) => s.streams)
  const hidden = useAppStore((s) => s.hiddenStreams)
  const toggleHidden = useAppStore((s) => s.toggleHidden)
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen)
  const logout = useAppStore((s) => s.logout)

  if (!creds) return null

  return (
    <div className="sheet" role="dialog" aria-label="Settings">
      <header className="topbar">
        <h2>Settings</h2>
        <button onClick={() => setSettingsOpen(false)}>Close</button>
      </header>
      <p className="identity">Signed in as {creds.email}</p>
      <h3>Visible projects</h3>
      <ul>
        {streams.map((s) => (
          <li key={s.stream_id}>
            <label>
              <input
                type="checkbox"
                checked={!hidden.includes(s.stream_id)}
                onChange={() => toggleHidden(s.stream_id)}
              />{' '}
              {s.name}
            </label>
          </li>
        ))}
      </ul>
      <button className="danger" onClick={logout}>Log out</button>
    </div>
  )
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/views/SettingsSheet.tsx src/views/SettingsSheet.test.tsx
git commit -m "feat: settings sheet with stream visibility and logout"
```

---

### Task 13: Event loop hook + App wiring

**Files:**
- Create: `src/hooks/useEventLoop.ts`
- Modify: `src/App.tsx` (replace placeholder), `src/App.test.tsx` (replace)
- Test: `src/hooks/useEventLoop.test.ts`, `src/App.test.tsx`

**Interfaces:**
- Consumes: `ZulipClient.register/pollEvents` (Task 6), `ZulipApiError` (Task 4), full store (Task 7), all views (Tasks 9–12), `loadCredentials` (Task 2).
- Produces: `useEventLoop(): void` — while `store.client` exists: register → applyInitialState → poll loop; `BAD_EVENT_QUEUE_ID` → silent re-register; 401 → `logout()`; other errors → `setConnection('offline')` + exponential backoff (1s doubling to max 30s); pauses while `document.visibilityState === 'hidden'`; stops on unmount/client change. Final `App` component.

- [ ] **Step 1: Write the failing hook test**

`src/hooks/useEventLoop.test.ts`:

```ts
import { renderHook } from '@testing-library/react'
import { waitFor } from '@testing-library/react'
import { useEventLoop } from './useEventLoop'
import { useAppStore, resetAppStore } from '../store/appStore'
import { ZulipApiError, type ZulipClient } from '../api/client'
import type { InitialState } from '../api/types'

const init: InitialState = {
  queueId: 'q1',
  lastEventId: 0,
  subscriptions: [{ stream_id: 1, name: 'alpha', description: 'A' }],
  unread: [],
}

const NEVER = new Promise<never>(() => {}) // parks the loop

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({
    creds: { prefix: '/zulip/qec', email: 'me@b.c', apiKey: 'k', sendTopic: '' },
  })
})

test('registers, applies initial state, applies polled events, goes live', async () => {
  const client = {
    register: vi.fn(async () => init),
    pollEvents: vi
      .fn()
      .mockResolvedValueOnce([
        { id: 1, type: 'message', message: {
          id: 5, sender_full_name: 'Bot', sender_email: 'bot@b.c',
          timestamp: 1755100000, content: '<p>x</p>', stream_id: 1, subject: '',
        } },
      ])
      .mockReturnValue(NEVER),
  } as unknown as ZulipClient
  useAppStore.setState({ client })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(useAppStore.getState().streams).toHaveLength(1))
  await waitFor(() => expect(useAppStore.getState().unreadByStream[1]).toEqual([5]))
  expect(useAppStore.getState().connection).toBe('live')
  unmount()
})

test('BAD_EVENT_QUEUE_ID triggers transparent re-register', async () => {
  const client = {
    register: vi.fn(async () => init),
    pollEvents: vi
      .fn()
      .mockRejectedValueOnce(new ZulipApiError('Bad event queue ID', 400, 'BAD_EVENT_QUEUE_ID'))
      .mockReturnValue(NEVER),
  } as unknown as ZulipClient
  useAppStore.setState({ client })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(client.register).toHaveBeenCalledTimes(2))
  unmount()
})

test('401 logs the user out', async () => {
  const client = {
    register: vi.fn(async () => {
      throw new ZulipApiError('Invalid API key', 401)
    }),
    pollEvents: vi.fn(),
  } as unknown as ZulipClient
  useAppStore.setState({ client })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
  unmount()
})

test('network errors set connection offline and back off', async () => {
  vi.useFakeTimers()
  const client = {
    register: vi.fn(async () => init),
    pollEvents: vi
      .fn()
      .mockRejectedValueOnce(new TypeError('Failed to fetch'))
      .mockReturnValue(NEVER),
  } as unknown as ZulipClient
  useAppStore.setState({ client })
  const { unmount } = renderHook(() => useEventLoop())
  await vi.waitFor(() => expect(useAppStore.getState().connection).toBe('offline'))
  await vi.advanceTimersByTimeAsync(1000)
  await vi.waitFor(() => expect(useAppStore.getState().connection).toBe('live'))
  unmount()
  vi.useRealTimers()
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npm test`
Expected: FAIL — cannot resolve `./useEventLoop`.

- [ ] **Step 3: Write the hook**

`src/hooks/useEventLoop.ts`:

```ts
import { useEffect } from 'react'
import { ZulipApiError } from '../api/client'
import { useAppStore } from '../store/appStore'

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function waitForVisible(): Promise<void> {
  return new Promise((resolve) => {
    const handler = () => {
      if (document.visibilityState === 'visible') {
        document.removeEventListener('visibilitychange', handler)
        resolve()
      }
    }
    document.addEventListener('visibilitychange', handler)
  })
}

export function useEventLoop(): void {
  const client = useAppStore((s) => s.client)

  useEffect(() => {
    if (!client) return
    let stopped = false
    const abort = new AbortController()
    let queue: { queueId: string; lastEventId: number } | null = null
    let backoff = 1000
    const store = useAppStore

    async function run() {
      while (!stopped) {
        if (document.visibilityState === 'hidden') {
          await waitForVisible()
          if (stopped) return
          continue
        }
        try {
          if (!queue) {
            store.getState().setConnection('connecting')
            const init = await client!.register()
            if (stopped) return
            store.getState().applyInitialState(init)
            queue = { queueId: init.queueId, lastEventId: init.lastEventId }
            store.getState().setConnection('live')
            backoff = 1000
            continue
          }
          const events = await client!.pollEvents(queue.queueId, queue.lastEventId, abort.signal)
          if (stopped) return
          for (const ev of events) {
            queue.lastEventId = Math.max(queue.lastEventId, ev.id)
          }
          store.getState().applyEvents(events)
          store.getState().setConnection('live')
          backoff = 1000
        } catch (e) {
          if (stopped) return
          if (e instanceof ZulipApiError && e.code === 'BAD_EVENT_QUEUE_ID') {
            queue = null
            continue
          }
          if (e instanceof ZulipApiError && e.httpStatus === 401) {
            store.getState().logout()
            return
          }
          store.getState().setConnection('offline')
          await sleep(backoff)
          backoff = Math.min(backoff * 2, 30000)
        }
      }
    }

    void run()
    return () => {
      stopped = true
      abort.abort()
    }
  }, [client])
}
```

- [ ] **Step 4: Replace App and its test**

`src/App.tsx`:

```tsx
import { useEffect } from 'react'
import { useAppStore } from './store/appStore'
import { loadCredentials } from './store/auth'
import { useEventLoop } from './hooks/useEventLoop'
import { LoginView } from './views/LoginView'
import { ProjectsView } from './views/ProjectsView'
import { ConversationView } from './views/ConversationView'
import { SettingsSheet } from './views/SettingsSheet'

export default function App() {
  const creds = useAppStore((s) => s.creds)
  const view = useAppStore((s) => s.view)
  const connection = useAppStore((s) => s.connection)
  const settingsOpen = useAppStore((s) => s.settingsOpen)

  useEffect(() => {
    if (!useAppStore.getState().creds) {
      const saved = loadCredentials()
      if (saved) useAppStore.getState().setCreds(saved)
    }
  }, [])

  useEventLoop()

  if (!creds) return <LoginView />

  return (
    <div className="app">
      {connection !== 'live' && (
        <div className="banner" role="status">Reconnecting…</div>
      )}
      {view.name === 'conversation' ? (
        <ConversationView streamId={view.streamId} />
      ) : (
        <ProjectsView />
      )}
      {settingsOpen && <SettingsSheet />}
    </div>
  )
}
```

`src/App.test.tsx` (replace entirely):

```tsx
import { render, screen } from '@testing-library/react'
import App from './App'
import { resetAppStore } from './store/appStore'

vi.mock('./api/servers', () => ({
  loadServers: vi.fn(async () => [{ name: 'QEC Harness', prefix: '/zulip/qec', sendTopic: '' }]),
}))

beforeEach(() => resetAppStore())

test('shows login when no credentials are stored', async () => {
  render(<App />)
  expect(await screen.findByRole('heading', { name: 'Agent Console' })).toBeInTheDocument()
  expect(screen.getByLabelText(/email/i)).toBeInTheDocument()
})
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `npm test`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/hooks/useEventLoop.ts src/hooks/useEventLoop.test.ts src/App.tsx src/App.test.tsx
git commit -m "feat: live event loop with reconnect/backoff and app wiring"
```

---

### Task 14: PWA — manifest, icons, service worker

**Files:**
- Create: `public/manifest.webmanifest`, `public/sw.js`, `public/icons/icon-180.png`, `public/icons/icon-192.png`, `public/icons/icon-512.png`
- Modify: `index.html`, `src/main.tsx`

**Interfaces:**
- Consumes: nothing from src.
- Produces: installable PWA. `sw.js` NEVER caches `/zulip/*` or `/servers.json`; network-first with cache fallback for everything else.

- [ ] **Step 1: Generate placeholder icons** (solid brand-color PNGs; replace with a real logo whenever one exists)

```bash
mkdir -p public/icons && python3 - <<'EOF'
import zlib, struct
def png_solid(w, h, rgb):
    def chunk(t, d):
        return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    ihdr = struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)
    raw = b''.join(b'\x00' + bytes(rgb) * w for _ in range(h))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', ihdr) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')
for size in (180, 192, 512):
    open(f'public/icons/icon-{size}.png', 'wb').write(png_solid(size, size, (79, 70, 229)))
EOF
```

- [ ] **Step 2: Write manifest and service worker**

`public/manifest.webmanifest`:

```json
{
  "name": "Agent Console",
  "short_name": "Agents",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#f8fafc",
  "theme_color": "#4f46e5",
  "icons": [
    { "src": "/icons/icon-192.png", "sizes": "192x192", "type": "image/png" },
    { "src": "/icons/icon-512.png", "sizes": "512x512", "type": "image/png" }
  ]
}
```

`public/sw.js`:

```js
const CACHE = 'zulip-app-v1'

self.addEventListener('install', () => self.skipWaiting())
self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()))

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url)
  if (e.request.method !== 'GET') return
  if (url.pathname.startsWith('/zulip/') || url.pathname === '/servers.json') return
  e.respondWith(
    fetch(e.request)
      .then((res) => {
        const copy = res.clone()
        caches.open(CACHE).then((c) => c.put(e.request, copy))
        return res
      })
      .catch(() => caches.match(e.request).then((hit) => hit ?? Response.error())),
  )
})
```

- [ ] **Step 3: Wire into index.html and main.tsx**

In `index.html` `<head>`, add after the `<title>`:

```html
    <link rel="manifest" href="/manifest.webmanifest" />
    <link rel="apple-touch-icon" href="/icons/icon-180.png" />
    <meta name="theme-color" content="#4f46e5" />
    <meta name="apple-mobile-web-app-capable" content="yes" />
    <meta name="apple-mobile-web-app-status-bar-style" content="default" />
```

In `src/main.tsx`, append at the end of the file:

```ts
if (import.meta.env.PROD && 'serviceWorker' in navigator) {
  navigator.serviceWorker.register('/sw.js')
}
```

- [ ] **Step 4: Verify build output contains PWA assets**

Run: `npm run build && ls dist/manifest.webmanifest dist/sw.js dist/icons`
Expected: all paths listed; `npm test` still green.

- [ ] **Step 5: Commit**

```bash
git add public index.html src/main.tsx
git commit -m "feat: PWA manifest, icons, and API-excluding service worker"
```

---

### Task 15: Deployment — Caddyfile + README

**Files:**
- Create: `deploy/Caddyfile`, `README.md`

**Interfaces:**
- Consumes: `public/servers.json` prefixes (Task 3) — the Caddyfile allowlist MUST cover exactly the prefixes listed there.
- Produces: deployable config + operator documentation.

- [ ] **Step 1: Write the Caddyfile**

`deploy/Caddyfile` (replace `app.example.com` with the real DNS name at deploy time):

```
app.example.com {
	encode zstd gzip
	root * /srv/zulip-app/dist

	# Allowlisted Zulip upstreams — one handle_path block per entry in
	# public/servers.json. NEVER add a catch-all proxy here: this must not
	# become an open relay.
	handle_path /zulip/qec/* {
		reverse_proxy https://qec-harness.zulipchat.com {
			header_up Host qec-harness.zulipchat.com
		}
	}

	handle {
		try_files {path} /index.html
		file_server
	}
}
```

- [ ] **Step 2: Write the README**

`README.md`:

```markdown
# Agent Console (Zulip Agent PWA)

A minimal installable web app for phones (Android + iOS) to read and send
messages in Zulip project streams that drive remote AI agents. One stream =
one project = one flat conversation. Spec:
`docs/superpowers/specs/2026-08-14-zulip-agent-pwa-design.md`.

## Develop

```sh
npm install
npm run dev        # http://localhost:5173 — /zulip/qec proxies to the real server
npm test           # unit + component tests
npm run e2e        # Playwright smoke (mocked API)
```

## Deploy

1. `npm run build` → static files in `dist/`.
2. Copy `dist/` to the host, e.g. `/srv/zulip-app/dist`.
3. Install Caddy; copy `deploy/Caddyfile` to `/etc/caddy/Caddyfile`, set the
   real domain, and `systemctl reload caddy`. HTTPS is automatic.
4. Friends open the URL, sign in with their Zulip email + password, and
   "Add to Home Screen".

## Adding another Zulip server

1. Add a `handle_path /zulip/<key>/*` block to `deploy/Caddyfile`
   pointing at the new upstream; reload Caddy.
2. Add a matching entry to `public/servers.json`
   (`{"name": "...", "prefix": "/zulip/<key>", "sendTopic": ""}`) and redeploy
   `dist/`.

The two lists must stay in sync. Never add a catch-all proxy.

## Notes

- No push notifications by design; the app only syncs while open.
- `sendTopic` is the topic every app-sent message uses (`""` = Zulip's
  "general chat" empty topic; set e.g. `"chat"` for older servers).
```

- [ ] **Step 3: Validate the Caddyfile if Caddy is installed locally** (skip without failing if not)

Run: `command -v caddy >/dev/null && caddy validate --config deploy/Caddyfile --adapter caddyfile || echo "caddy not installed — validated at deploy time"`
Expected: `Valid configuration` or the skip message.

- [ ] **Step 4: Commit**

```bash
git add deploy/Caddyfile README.md
git commit -m "docs: deployment config and operator README"
```

---

### Task 16: Playwright smoke test + CI

**Files:**
- Create: `playwright.config.ts`, `e2e/smoke.spec.ts`, `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: the whole app; route-mocks every network call, so no real Zulip server is needed.
- Produces: `npm run e2e` green locally and in GitHub Actions.

- [ ] **Step 1: Write the Playwright config**

`playwright.config.ts`:

```ts
import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  use: { baseURL: 'http://localhost:5173' },
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:5173',
    reuseExistingServer: !process.env.CI,
  },
})
```

- [ ] **Step 2: Write the smoke test**

`e2e/smoke.spec.ts`:

```ts
import { test, expect } from '@playwright/test'

const message = {
  id: 7,
  sender_full_name: 'Research Agent',
  sender_email: 'bot@qec.example',
  timestamp: 1755100000,
  content: '<p>Scan complete: <strong>3 candidates</strong> found.</p>',
  stream_id: 1,
  subject: '',
}

test('login → projects → conversation → send', async ({ page }) => {
  let eventCalls = 0
  await page.route('**/servers.json', (r) =>
    r.fulfill({ json: [{ name: 'QEC Harness', prefix: '/zulip/qec', sendTopic: '' }] }),
  )
  await page.route('**/zulip/qec/api/v1/fetch_api_key', (r) =>
    r.fulfill({ json: { result: 'success', api_key: 'k1', email: 'me@qec.example' } }),
  )
  await page.route('**/zulip/qec/api/v1/register', (r) =>
    r.fulfill({
      json: {
        result: 'success',
        queue_id: 'q1',
        last_event_id: -1,
        subscriptions: [{ stream_id: 1, name: 'qec', description: 'QEC research project' }],
        unread_msgs: { streams: [{ stream_id: 1, topic: '', unread_message_ids: [7] }] },
      },
    }),
  )
  await page.route('**/zulip/qec/api/v1/events**', async (r) => {
    eventCalls += 1
    if (eventCalls === 1) {
      await r.fulfill({ json: { result: 'success', events: [{ id: 0, type: 'heartbeat' }] } })
    }
    // later polls: leave pending to simulate a long-poll parked at the server
  })
  await page.route('**/zulip/qec/api/v1/messages?**', (r) =>
    r.fulfill({ json: { result: 'success', messages: [message] } }),
  )
  await page.route('**/zulip/qec/api/v1/messages', (r) =>
    r.fulfill({ json: { result: 'success', id: 42 } }),
  )
  await page.route('**/zulip/qec/api/v1/mark_stream_as_read', (r) =>
    r.fulfill({ json: { result: 'success' } }),
  )

  await page.goto('/')
  await page.getByLabel(/email/i).fill('me@qec.example')
  await page.getByLabel(/password/i).fill('pw')
  await page.getByRole('button', { name: /sign in/i }).click()

  await expect(page.getByText('qec')).toBeVisible()
  await page.getByRole('button', { name: /qec/ }).click()

  await expect(page.getByText('Scan complete:')).toBeVisible()
  await page.getByRole('textbox').fill('run the next batch')
  await page.getByRole('button', { name: /^send$/i }).click()
  await expect(page.getByRole('textbox')).toHaveValue('')
})
```

- [ ] **Step 3: Run the smoke test**

Run: `npx playwright install chromium && npm run e2e`
Expected: 1 passed.

- [ ] **Step 4: Write the CI workflow**

`.github/workflows/ci.yml`:

```yaml
name: CI
on:
  push:
  pull_request:

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
      - run: npm ci
      - run: npm test
      - run: npm run build
      - run: npx playwright install --with-deps chromium
      - run: npm run e2e
```

- [ ] **Step 5: Commit**

```bash
git add playwright.config.ts e2e/smoke.spec.ts .github/workflows/ci.yml
git commit -m "test: Playwright smoke flow and CI workflow"
```

---

## Post-plan manual checklist (not tasks — done by the owner at release time)

Per the spec's testing section, once deployed to a real host:

1. Log in against the real `qec-harness.zulipchat.com` realm through the proxy; capture 3–5 real rendered-message HTML samples and paste them into `src/test/fixtures/zulipHtml.ts` (replacing the hand-written approximations), re-run `npm test`.
2. Verify empty-topic send is accepted by the realm; if rejected, set `"sendTopic": "chat"` in `public/servers.json`.
3. Install to home screen on one Android and one iPhone; check safe-area rendering, keyboard-over-composer, and background → foreground resume (event queue re-register).
