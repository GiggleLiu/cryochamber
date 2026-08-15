You are implementing two approved features in the Agent Console PWA (repo root = cwd, branch main, TDD required, three commits). Read existing code first: `src/api/client.ts`, `src/store/appStore.ts`, `src/components/Composer.tsx`, `src/components/MessageBody.tsx`, `src/views/ConversationView.tsx`, `src/styles.css`, and their tests. Do not touch unrelated code.

## Feature 1 — @-mention support (commits: "feat: mention rendering with self-highlight" and "feat: composer @-mention autocomplete")

### API additions (src/api/client.ts)
- `getUsers(): Promise<ZulipUser[]>` — GET `/users`; response `{members: [...]}`; return active members (`is_active !== false`) mapped to `ZulipUser { user_id: number; full_name: string; email: string; is_bot: boolean }` (add the type to src/api/types.ts).
- `getOwnUser(): Promise<{ user_id: number }>` — GET `/users/me`; return `{ user_id }`.
- Tests: URL/auth-header correctness, member mapping, inactive filtered out.

### Store additions (src/store/appStore.ts)
- Fields `users: ZulipUser[] | null` (null = not yet fetched) and `ownUserId: number | null`; actions `setUsers`, `setOwnUserId`; both reset on logout. Tests.

### Rendering (src/components/MessageBody.tsx + ConversationView + styles.css)
- New optional prop `selfUserId?: number`. In the existing rewrite pass: for every `span.user-mention` / `span.user-group-mention`, if `data-user-id` equals `String(selfUserId)`, add class `mention-me`.
- ConversationView passes `selfUserId={ownUserId ?? undefined}` from the store, and on mount (once, if `ownUserId === null`) calls `client.getOwnUser()` → `setOwnUserId` (ignore errors silently except auth errors → existing logout path).
- CSS scoped in .message-body: `.user-mention, .user-group-mention { color: #576b95; background: rgba(87,107,149,0.12); border-radius: 4px; padding: 0 0.15em; }` and `.mention-me { background: #ffd666; color: #7a4d00; }`.
- Note: `data-user-id` must remain in the sanitizer's ALLOWED_ATTR (add it if absent).
- Tests: mention span keeps classes and data-user-id; selfUserId match adds mention-me; non-match doesn't.

### Composer autocomplete (src/components/Composer.tsx + styles.css)
- On textarea change, examine text before the caret: if it matches `/@([\p{L}\p{N}_ ]*)$/u`, open a panel (`div.mention-panel`, absolutely positioned above the composer) listing up to 8 users whose `full_name` case-insensitively includes the query (prefix matches first). First open triggers a lazy `client.getUsers()` → `setUsers` (cache; show nothing while loading; on error close silently).
- Selection: ArrowUp/ArrowDown move an active row (`.active`), Enter or Tab confirms (preventDefault only while panel open), Escape closes, click/tap confirms. Confirming replaces the `@query` before the caret with `@**Full Name** ` (Zulip canonical syntax, trailing space) and closes the panel.
- Tests (userEvent): typing `@al` filters to matching users; Enter inserts `@**Alice Doe** ` replacing the partial; Escape closes without inserting; panel doesn't open mid-word without `@`; send button behavior unchanged.

## Feature 2 — file download/upload (commit: "feat: attachment download and composer file upload")

### Download (src/components/MessageBody.tsx)
- Replace the current upload-anchor click behavior (authenticated fetch then `window.open`) with a true download: after the authenticated fetch resolves, create `<a href=blobUrl download=<filename>>`, click it, then revoke the blob URL. Filename = last path segment of the href, URL-decoded. While fetching, set the anchor text to `Downloading…` and restore after. Keep images' blob-swap behavior untouched.
- Update the existing anchor tests to assert the download path (an anchor with download attribute clicked / no window.open call).

### Upload (src/api/client.ts + src/components/Composer.tsx + styles.css)
- `uploadFile(file: File): Promise<string>` — POST `/user_uploads` with a `FormData` body (field name `file`); do NOT set a Content-Type header manually (browser sets the multipart boundary — verify the shared request() helper doesn't force one; adjust the call to bypass FORM headers). Return `body.uri ?? body.url` (server version differences). Test: FormData body, no explicit Content-Type, Authorization present, uri/url fallback, error mapping via ZulipApiError.
- Composer: a paperclip button (`aria-label="Attach file"`) + hidden `<input type="file">`. On pick: disable composer send, show `Uploading <name>…` status line; on success insert `[<name>](<uri>)` at the caret (space-separated from surrounding text); on failure show the existing error line style with the server message; re-enable either way. Reset the input so the same file can be re-picked.
- Tests: successful upload inserts the markdown link; failed upload shows error and leaves text unchanged; send disabled during upload.

## Gate before finishing
`npm test` all green, `npm run build` clean, `npm run e2e` passing (adjust smoke locators only if markup changes force it). No subagents or other AI tools. If DOM APIs fight jsdom (e.g. caret APIs), prefer logic extracted into pure helpers you can unit-test, and report any residual gap honestly.

## Report
Full report to `.superpowers/followup/mentions-files-report.md` (per-feature summary, TDD RED/GREEN evidence, files changed, commands + output, self-review, concerns). End your final console message with ONLY (≤10 lines): Status; Commits; one-line test summary; concerns.
