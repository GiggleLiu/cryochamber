# Cryochamber — native shell

A [Tauri v2](https://tauri.app) window that loads the **unchanged** Agent
Console bundle (`console/dist`) and gives it three things a browser cannot:

- **Native hub persistence.** Hub accounts — URL, bearer token, label, trust
  record — live in a JSON file in the app's private data directory
  (`tauri-plugin-store`), not in `localStorage`. They survive a WebView data
  eviction; a browser profile wipe does not cost the user their tokens.
- **Native transport.** Every hub request goes out through the OS, not the
  WebView's fetch, so there is no origin and therefore no CORS: one window can
  talk to many hubs at once, and a hub on plain `http://` is reachable at all.
- **Trust the user decides.** A hub whose certificate the system does not
  trust is not silently accepted and not silently refused: the shell probes it,
  shows the SHA-256 fingerprint, and — if the user confirms it against what the
  operator read out — pins that exact certificate for that hub from then on.

The console itself is unaware of any of this. It reaches the shell through
`window.__TAURI__` behind an `isTauri()` check (`app.withGlobalTauri = true`),
so **no npm dependency is added to `console/package.json`** and browser mode is
byte-for-byte the bundle `cryohub` already serves.

| | |
| --- | --- |
| Bundle identifier | `com.cryochamber.console` |
| Hub store (macOS) | `~/Library/Application Support/com.cryochamber.console/hubs.json` |
| Crate | `app/src-tauri` — a **standalone** crate, deliberately not a member of the root `cryochamber` package (`cargo build` at the repo root never compiles it, and `cargo package` never ships it) |

The store file holds bearer tokens in cleartext, protected by the OS file
permissions on the app's data directory. Treat it the way you treat
`~/.config/cryo/cryohub-tokens.json`.

## Prerequisite

The Tauri CLI is not vendored — install it once:

```bash
cargo install tauri-cli --version '^2'   # provides `cargo tauri`
```

Node is needed too (the console build), plus Xcode command line tools for the
macOS bundle.

## Make targets

Run them from the repository root.

| Target | What it does |
| --- | --- |
| `make app-dev` | Opens the shell against the Vite dev server (`console` on `:5173`, `strictPort`) with hot reload. The port is strict on purpose: a stale dev server on `:5173` would silently serve a *different* console into the window, so an occupied port fails the run instead. |
| `make app-macos` | `make console-build`, then `cargo tauri build`. Produces `app/src-tauri/target/release/bundle/macos/Cryochamber.app` and `app/src-tauri/target/release/bundle/dmg/Cryochamber_<version>_aarch64.dmg`. |
| `make app-android` | `cargo tauri android build --apk --target aarch64`. See [Android release](#android-release) below for the toolchain and signing setup. |
| `make app-check` | `cargo fmt --check` + `cargo clippy --all-targets -D warnings` + `cargo test`, run inside `app/src-tauri`. The console's own suite is `make console-check`. |

The dmg is **ad-hoc signed** — Tauri's default without a signing identity.
Notarization is deferred (see the spec's §6), so first launch needs the
right-click gesture in item 5 below.

## Release smoke checklist

The multi-hub model itself is covered by vitest against injected fetch fakes.
What no unit test reaches is the Tauri seam — the plugin transport, the native
store, and the trust prompts — so **run this list by hand against the dmg
build, not `make app-dev`**, before every release. The dev shell loads a
different frontend over a different transport path; a bug in the bundled
console will not show up there.

Set up once:

```bash
make app-macos
open app/src-tauri/target/release/bundle/dmg/   # then drag Cryochamber.app to /Applications
```

Start from an empty state — if you have run the app before, quit it and remove
`~/Library/Application Support/com.cryochamber.console/hubs.json` so item 4
tests real persistence rather than a leftover file.

### 1. Add a plain-http hub — the warning appears and gates the button

Start a hub on loopback and print its owner token:

```bash
cryohub start                # binds 127.0.0.1:8765 by default
cryohub token owner          # prints the bearer token; paste it into the app
```

In the app's Add chamber screen, enter `http://127.0.0.1:8765` and the token.

- [ ] A warning appears as soon as the address is plain HTTP: *"This hub is
      plain HTTP. The token and every message you send it travel unencrypted…"*
- [ ] **Add chamber** stays disabled until the *"I understand traffic to this hub is
      unencrypted"* checkbox is ticked.
- [ ] Editing the address clears the tick (the acknowledgement is about one
      host, not about the form).
- [ ] With the box ticked, the hub is added and its chambers load.

### 2. Add a self-signed https hub — the pin sheet shows the right fingerprint

No public CA is involved, so put a TLS terminator with a self-signed
certificate in front of the loopback hub. `socat` is the shortest route
(`brew install socat`); `caddy` with an explicit `tls <cert> <key>` works the
same way.

```bash
cd "$(mktemp -d)"
# A certificate for localhost, valid a month, no CA anywhere:
openssl req -x509 -newkey rsa:2048 -nodes -days 30 \
  -keyout hub-key.pem -out hub-cert.pem \
  -subj "/CN=localhost" -addext "subjectAltName=DNS:localhost,IP:127.0.0.1"
cat hub-cert.pem hub-key.pem > hub.pem
# Terminate TLS on :8443 and forward the plaintext to the hub on :8765:
socat OPENSSL-LISTEN:8443,cert=hub.pem,verify=0,reuseaddr,fork TCP:127.0.0.1:8765
```

Read the fingerprint the way the hub operator would, so the comparison is a
real out-of-band check and not a copy of the same screen:

```bash
openssl s_client -connect localhost:8443 </dev/null 2>/dev/null \
  | openssl x509 -fingerprint -sha256 -noout
# sha256 Fingerprint=88:44:DD:65:…
```

Now add `https://localhost:8443` with the same token.

- [ ] An **Untrusted certificate** sheet appears instead of the hub being added.
- [ ] The fingerprint it shows matches the `openssl` output above, group for
      group (the sheet formats it as colon-separated uppercase hex for exactly
      this comparison).
- [ ] **Cancel** adds nothing — the hub does not appear in the list.
- [ ] **Add chamber anyway** adds it, and the chamber list, message stream, and
      image attachments all load over the pinned connection.
- [ ] Restart `socat` with a *freshly generated* certificate (repeat the
      `openssl req` line) and reopen the app: the hub now fails to connect
      rather than silently trusting the new certificate. Re-pin by removing and
      re-adding the hub.

### 3. Two hubs live — owned/joined lists and one hub dying

Keep the hub from item 1 and add a second. A real hub on another host is the
most faithful test; a second `cryohub` on this machine works too, given its own
config and state directories so it does not fight the first over
`cryohub.toml`, the token store, and the `hub` service label:

```bash
mkdir -p /tmp/hub2/config /tmp/hub2/state
XDG_CONFIG_HOME=/tmp/hub2/config XDG_STATE_HOME=/tmp/hub2/state \
  cryohub start --foreground --port 8766
# in another shell, for its token:
XDG_CONFIG_HOME=/tmp/hub2/config XDG_STATE_HOME=/tmp/hub2/state cryohub token owner
```

`--foreground` keeps it out of launchd, so Ctrl-C is the kill in the step
below and nothing is left installed afterwards.

- [ ] The chamber list shows chambers from **both** hubs under **Owned** and
      **Joined** according to each saved token.
- [ ] Every row carries a hub chip naming its hub (chips only appear once more
      than one hub is configured — a chip repeating the same word on every row
      would be noise).
- [ ] Open a conversation on each hub: messages, sending, and attachments work
      per hub, and unread counts are namespaced per hub (no bleed).
- [ ] Now kill one hub (`cryohub stop`, or Ctrl-C the `socat` in front of it).
      Within a few seconds its rows' chips read **"· unreachable"** and go
      muted — the row is still the last thing that hub said, not a blank.
- [ ] **The other hub keeps streaming.** Send a message on the live hub while
      the dead one is down; it goes through, and its chamber list keeps
      updating. One hub down is not the app down.
- [ ] Restart the killed hub: its rows recover to normal on their own, with no
      app restart and no re-entry of the token.
- [ ] Add another token for a hub already present. Its chambers appear without
      removing any chamber from the first token's scope.

### 4. Cold restart — hubs and tokens persist, caches hydrate

- [ ] Quit the app entirely (⌘Q — not just closing the window) and reopen it.
- [ ] Both hubs are still there with their labels; **no token is asked for
      again**, including the pinned hub's pin.
- [ ] The chamber list and the last conversation's messages appear immediately
      from cache, before the network answers, then refresh in place.
- [ ] `~/Library/Application Support/com.cryochamber.console/hubs.json` exists
      and lists both hubs under a `hubs` key, each with the trust decision it
      was added with — `{"kind":"https"}`, `{"kind":"plain-http"}`, or
      `{"kind":"pinned","sha256":"…"}` carrying the fingerprint from item 2.

### 5. The unsigned dmg opens with right-click → Open

Test this on a machine that has **not** built the app — Gatekeeper's quarantine
flag comes from the download, so a locally built bundle will not reproduce it.
Upload the dmg somewhere and download it back, or `xattr -w
com.apple.quarantine "0081;00000000;Safari;" /path/to/Cryochamber.app`.

- [ ] Double-clicking the app is refused ("cannot be opened because the
      developer cannot be verified" / "damaged").
- [ ] **Right-click (or Control-click) the app → Open → Open** launches it, and
      subsequent launches work normally by double-click.
- [ ] The release notes say this, and say that notarization is deferred.

The signature state to expect:

```console
$ codesign -dv Cryochamber.app 2>&1 | grep -i signature
Signature=adhoc
```

## Android release

The generated Gradle project is committed under `app/src-tauri/gen/android`.
Tagged releases build one signed arm64 APK and attach it to the GitHub release.
The app allows plain HTTP because adding such a hub requires an explicit warning
and acknowledgement.

### Signing identity

The maintainer copy is outside the repository:

- keystore: `~/.config/cryochamber/android-release.jks`
- alias: `cryochamber`
- password: macOS Keychain service
  `com.cryochamber.console.android-keystore`
- CI copies: `ANDROID_KEYSTORE_B64`, `ANDROID_KEY_ALIAS`, and
  `ANDROID_KEYSTORE_PASSWORD` GitHub Secrets

Back up the keystore file somewhere outside this machine. Never replace it:
Android accepts an APK as an update only when it has the same signer, and
uninstalling the old app discards its hub store.

### Local build

A local Android build needs JDK 17, Android SDK 36, NDK
`29.0.13846066`, the Tauri CLI, and the arm64 Rust target:

```bash
rustup target add aarch64-linux-android
make app-android
```

To exercise release signing locally, create the ignored
`app/src-tauri/gen/android/keystore.properties`:

```properties
keyAlias=cryochamber
password=<password from macOS Keychain>
storeFile=/absolute/path/to/.config/cryochamber/android-release.jks
```

Then run `make app-android` and verify the result with
`apksigner verify --print-certs <apk>`. Delete
`keystore.properties` afterward.

### First-device check

Before announcing the first APK:

- Install it on an arm64 device running Android 7 or later.
- Add one plain-HTTP hub and one HTTPS hub.
- Open a conversation containing rendered math and an image attachment.
- Send a message, restart the app, and confirm both hubs remain.
- Check `adb logcat` or `chrome://inspect` for CSP errors.
