---
name: screenshot-cryohub
description: Use when the user wants a screenshot of the cryohub web dashboard for documentation, README, mdbook, or a slide deck. Spawns cryohub on a chamber workspace, drives headless Chrome, and saves a PNG to the requested path.
---

# Screenshot the cryohub dashboard

Capture a deterministic PNG of the cryohub web UI without disturbing the user's running Chrome session or any cryohub instance they already have on the default port.

The mechanical work — launching cryohub on a non-default port, waiting for it to be reachable, resolving a chamber id, driving headless Chrome with a fresh `--user-data-dir`, and tearing everything down on exit — lives in `screenshot.sh` next to this file. Your job is to gather the inputs, run the script, and verify the result.

## When to use

- The user asks for a screenshot of the dashboard, hub UI, or "what cryohub looks like".
- Refreshing an existing screenshot in `docs/src/images/` after a UI change.
- Generating illustrations for a blog post, README, or mdbook page.

## Inputs to confirm before running

Ask the user only if not specified:

1. **Workspace directory** (`--workspace`) — a directory whose immediate subdirectories are chambers (each with a `cryo.toml`). Default: `examples/chambers`.
2. **Selected chamber** (`--chamber`) — which chamber's detail pane should be visible. Default: `mr-lazy` (rich message history). Pass `none` to capture the empty-state landing.
3. **Output path** (`--output`) — where to write the PNG. Default: `docs/src/images/cryohub-dashboard.png`.
4. **Window size** (`--size`) — default `1440x900`. Use `1280x720` for compact docs, `1920x1080` for slides.
5. **Port** (`--port`) — default `18765`. Only change it if the user has a process on that port. Never use `8765` (the cryohub default — likely the user's real hub).

## Step 1: Run the script

From the repo root:

```bash
.claude/skills/screenshot-cryohub/screenshot.sh \
  --workspace examples/chambers \
  --chamber mr-lazy \
  --output docs/src/images/cryohub-dashboard.png \
  --size 1440x900 \
  --port 18765
```

The script:
- verifies Chrome (macOS `/Applications/Google Chrome.app/...`, Linux `google-chrome`/`chromium`/`chromium-browser`) and `cryohub` (auto-builds via `cargo build --bin cryohub` if missing);
- launches `cryohub start --foreground` from the workspace dir on the chosen port;
- polls the hub URL until it responds (or fails fast if the hub exits early);
- when `--chamber` is not `none`, looks up the id via `/api/chambers`, POSTs to `/api/chambers/<id>/start`, and uses `/c/<id>` as the URL;
- runs headless Chrome (`--headless=old`, fresh `--user-data-dir=/tmp/chrome-shot-$$`, scrollbars hidden) and writes the PNG;
- on exit (success or failure) stops any chamber it started and kills the hub.

On success it prints a single line like:

```
ok: wrote docs/src/images/cryohub-dashboard.png (123456 bytes, 1440x900, chamber=mr-lazy)
```

On failure it prints `ERROR: ...` to stderr and dumps the cryohub log. Common causes are listed in **Pitfalls** below.

## Step 2: Verify the screenshot

Use the `Read` tool on the PNG path to view the image and confirm it shows what you expect (sidebar populated, chamber selected, no spinner). If it shows a blank or partially loaded UI, re-run the script — first paint usually settles in <2s but SSE-driven panels occasionally need a second pass.

## Pitfalls (what the script protects against — so you don't reintroduce them)

- **Don't run raw `cryohub start` on `8765`.** That's the user's real hub. The script defaults to `18765` and lets you override.
- **Don't share a Chrome profile dir.** A shared `--user-data-dir` collides with the user's running Chrome via the singleton lock. The script always uses `/tmp/chrome-shot-$$` and removes it on exit.
- **Don't use `--headless=new` on macOS.** It hangs intermittently when another Chrome is running. The script uses `--headless=old`.
- **Don't `pkill -f "Google Chrome"`.** That nukes the user's real Chrome. The script tracks the hub pid and the temp user-data-dir and tears down only its own processes.
- **Trust the file, not Chrome's exit code.** Chrome can emit non-zero exits even after writing the PNG; the script checks `[ -s "$OUTPUT" ]` instead.
- **Don't re-encode the chamber id.** The id from `/api/chambers` is already URL-safe; the script interpolates it as-is.

If you hit something the script doesn't handle, fix it in `screenshot.sh` rather than copy-pasting bash into the conversation.

## Output format

Tell the user:

1. The path that was written.
2. The window size.
3. Whether a chamber was selected, and which one.
4. A reminder to embed the image with descriptive alt text, e.g.

   ```markdown
   ![cryohub dashboard with the mr-lazy chamber selected](./images/cryohub-dashboard.png)
   ```
