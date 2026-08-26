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

Run all three from the repository root.

| Target | What it does |
| --- | --- |
| `make app-dev` | Opens the shell against the Vite dev server (`console` on `:5173`, `strictPort`) with hot reload. The port is strict on purpose: a stale dev server on `:5173` would silently serve a *different* console into the window, so an occupied port fails the run instead. |
| `make app-macos` | `make console-build`, then `cargo tauri build`. Produces `app/src-tauri/target/release/bundle/macos/Cryochamber.app` and `app/src-tauri/target/release/bundle/dmg/Cryochamber_<version>_aarch64.dmg`. |
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

In the app's Add Hub screen, enter `http://127.0.0.1:8765` and the token.

- [ ] A warning appears as soon as the address is plain HTTP: *"This hub is
      plain HTTP. The token and every message you send it travel unencrypted…"*
- [ ] **Add hub** stays disabled until the *"I understand traffic to this hub is
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
- [ ] **Add hub anyway** adds it, and the chamber list, message stream, and
      image attachments all load over the pinned connection.
- [ ] Restart `socat` with a *freshly generated* certificate (repeat the
      `openssl req` line) and reopen the app: the hub now fails to connect
      rather than silently trusting the new certificate. Re-pin by removing and
      re-adding the hub.

### 3. Two hubs live — merged list, chips, and one hub dying

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

- [ ] The chamber list shows chambers from **both** hubs, merged and sorted
      together.
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

## Android release: first-time setup (maintainer)

The Android project and its CI job should land together after a maintainer with
the Android toolchain completes these steps. Until then, releases contain only
the macOS app.

### 1. Prerequisites

- Android Studio, or the command-line tools, with **SDK 34+** and **NDK 27+**.
- **JDK 17** — the version CI uses (`temurin` 17).
- `ANDROID_HOME` and `NDK_HOME` exported and pointing at them.
- All four Android Rust targets, even though the release builds only arm64:
  `tauri-cli` checks for the whole set before it starts.

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi \
  i686-linux-android x86_64-linux-android
```

Plus the Tauri CLI from the [Prerequisite](#prerequisite) section above.

### 2. Generate the Android project

```bash
cd app/src-tauri && cargo tauri android init
```

This writes `app/src-tauri/gen/android` — a Gradle project that gets
**committed**. Its `applicationId` is **`com.cryochamber.console`**, taken from
`tauri.conf.json`'s `identifier`. (An early plan draft said
`com.cryochamber.app`; that is wrong — the id must match the identifier.)

### 3. Narrow `app/.gitignore`

`app/.gitignore` currently ignores `src-tauri/gen/` wholesale, which would
swallow the project you just generated. **Replace that one line** with the
rules below. A `!` negation would not work here: git does not descend into an
ignored directory, so re-including files under it has no effect — the ignore
has to be narrowed rather than undone.

```
src-tauri/target/
src-tauri/gen/schemas/
src-tauri/gen/apple/
src-tauri/gen/android/.gradle/
src-tauri/gen/android/app/build/
src-tauri/gen/android/build/
src-tauri/gen/android/local.properties
src-tauri/gen/android/keystore.properties
*.jks
```

### 4. Allow cleartext traffic in the manifest

In `app/src-tauri/gen/android/app/src/main/AndroidManifest.xml`, add to the
`<application>` element:

```xml
android:usesCleartextTraffic="true"
```

with the reason above it, so nobody later "fixes" it:

```xml
<!-- Plain-http hubs are a first-class, user-confirmed case (LAN hubs have no
     TLS). The app's own per-hub trust sheet is the control; the platform
     default would silently break exactly the hubs this app exists for. -->
```

### 5. Wire Gradle release signing

`cargo tauri android init` does **not** generate a signing config — add one by
hand to `app/src-tauri/gen/android/app/build.gradle.kts`. Every part of it is
guarded on `keystore.properties` existing, so a contributor without the
keystore still builds debug-signed. The release CI job can use the same config
when it lands.

At the top of the file:

```kotlin
import java.io.FileInputStream
import java.util.Properties

val keystorePropertiesFile = rootProject.file("keystore.properties")
val keystoreProperties = Properties().apply {
    if (keystorePropertiesFile.exists()) load(FileInputStream(keystorePropertiesFile))
}
```

Inside `android { }`:

```kotlin
    signingConfigs {
        if (keystorePropertiesFile.exists()) {
            create("release") {
                keyAlias = keystoreProperties["keyAlias"] as String
                keyPassword = keystoreProperties["password"] as String
                storeFile = file(keystoreProperties["storeFile"] as String)
                storePassword = keystoreProperties["password"] as String
            }
        }
    }
```

And inside the existing `buildTypes { getByName("release") { ... } }`:

```kotlin
            if (keystorePropertiesFile.exists()) {
                signingConfig = signingConfigs.getByName("release")
            }
```

### 6. Create the release keystore

Once, locally, never committed:

```bash
keytool -genkey -v -keystore ~/cryochamber-release.jks -keyalg RSA -keysize 2048 \
  -validity 10000 -alias cryochamber
```

The password must contain **no newline and no backslash**: it travels through a
Java `.properties` file (where `\` is an escape character) and through CI's
`printf`, and either one would quietly turn it into a different password.

**Back the file up**, in a password manager alongside the store password and
the alias. This key signs every release from now on; Android treats a new APK
as an upgrade only if the signer is identical, so losing the key forces every
user to uninstall — discarding their hub store and tokens — before they can
install again.

### 7. Set the GitHub secrets

```bash
base64 -i ~/cryochamber-release.jks | gh secret set ANDROID_KEYSTORE_B64
gh secret set ANDROID_KEYSTORE_PASSWORD    # prompts; paste the store password
gh secret set ANDROID_KEY_ALIAS --body cryochamber
```

The future Android release job will require all three.

To exercise the signing path locally before trusting CI with it, write
`app/src-tauri/gen/android/keystore.properties`:

```
keyAlias=cryochamber
password=<store password>
storeFile=<absolute path to cryochamber-release.jks>
```

then `cd app/src-tauri && cargo tauri android build --apk --target aarch64` and
confirm `apksigner verify --print-certs <apk>` names the cryochamber
certificate. Delete `keystore.properties` afterwards and check `git status` is
clean — both it and `*.jks` are covered by step 3's rules.

### 8. Verify the CSP on a real device

Two directives in the shell's content-security-policy take effect only on
Android and have therefore **never executed on a device**. Check them on the
first Android run, before the first release:

- [ ] Sign in to a hub from the APK.
- [ ] Open a conversation containing **both** rendered math and an image
      attachment — the two features that pull in the fonts, styles, and blob
      URLs the policy governs.
- [ ] Watch `adb logcat`, or attach `chrome://inspect` to the WebView, and
      confirm there is **not one** `Refused to …` line. Any such line is a
      directive that needs widening before the release, not after.

### 9. Rehearse the release before the real one

Do not let the first APK CI ever builds be a real release:

1. Push an `rc` tag on a fork, or temporarily widen `release.yml`'s tag filter
   on a branch, so the workflow runs end to end.
2. Let CI produce the APK and attach it.
3. Sideload it onto a device and run the [Release smoke
   checklist](#release-smoke-checklist) above against it — the Tauri seam it
   covers is untested by anything else, and the APK is a build no one has run.
4. Add the Android release job with the exact `tauri-cli` version used in the
   rehearsal.
