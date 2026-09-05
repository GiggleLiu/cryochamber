# Installing the app

The **Cryochamber app** is a native window around the same [Agent
Console](./agent-console.md) a hub serves in a browser. It exists for one
reason a browser cannot cover: it holds **several access links at once**, groups
their chambers under Owned and Joined, and reaches each hub over the OS network stack rather
than the page's own origin.

Nothing about the hub changes. The console a hub serves stays exactly where it
was — open `http://127.0.0.1:8765` in a browser, or *Add to Home Screen* it as
a PWA, and you get the same surface bound to that one hub. The app is the
option for when *one* is no longer the number of hubs you have.

Release v0.2.8 predates the native app and has no installers. Until a newer
[GitHub release](https://github.com/GiggleLiu/cryochamber/releases) includes them,
follow the [source build instructions](https://github.com/GiggleLiu/cryochamber/blob/main/app/README.md).
The release workflow produces these filenames:

| File | For |
|---|---|
| `cryochamber-vX.Y.Z-android-arm64.apk` | 64-bit ARM Android phones and tablets |
| `cryochamber-vX.Y.Z-macos-arm64.dmg` | macOS on Apple Silicon |
| `cryochamber-vX.Y.Z-macos-arm64.app.zip` | the same app, zipped, if you would rather not mount a disk image |

On macOS take the **dmg**. Both macOS files carry the identical `.app`,
re-sealed with a full `codesign --deep -s -` before packaging; the `.app.zip`
exists only for anyone who would rather not mount a disk image.

There is no Play Store, App Store, Windows, or Intel Mac build. What is above
is what exists.

## Android

1. Download `cryochamber-vX.Y.Z-android-arm64.apk` from the release page.
2. Open it and allow *Install unknown apps* for the browser or file manager when
   Android asks. Return to the APK and install it.
3. Enter a hub address and access token, or paste an admin or invite link into
   *Admin or invite link*.
4. Cryochamber links open in the app. For an ordinary web invite link, use
   Android's **Share** action and choose Cryochamber.

The APK supports arm64 devices running Android 7 or later. It is signed, but it
is distributed directly through GitHub rather than Google Play.

## macOS

1. Download `cryochamber-vX.Y.Z-macos-arm64.dmg`, open it, drag **Cryochamber**
   to *Applications*.
2. **The first launch needs a right-click.** The build is ad-hoc signed and
   **not notarized**, so double-clicking it gets you *"cannot be opened because
   the developer cannot be verified"*. Right-click (or Control-click) the app in
   *Applications* → **Open** → **Open**. macOS remembers the decision;
   afterwards it launches normally. On **macOS 15 (Sequoia) and later** that
   right-click bypass is gone — let the first launch be refused, then approve
   the app under *System Settings → Privacy & Security → **Open Anyway***.
3. Enter a hub address and access token, or paste an admin or invite link into
   *Admin or invite link*.

Apple Silicon only. Notarization is not done yet, and this page will say so
until it is.

## Trust: what the app asks before it sends your token

An access token is a password. Before the app sends one to an address, it
decides — visibly — how much that address can be trusted. There are three
cases.

**HTTPS with a certificate your system already trusts.** Nothing is asked.
This is a hub behind a reverse proxy with a real certificate ([Caddy, as in the
console guide](./agent-console.md#public-deployment-phone-outside-your-network))
and it is the case to aim for.

**Plain `http://`.** A warning appears under the address as soon as you type
one, and **Add chamber** stays disabled until you tick *"I understand traffic to
this hub is unencrypted"*. What it means literally: the token and every message
you send travel readable by anything between the device and the hub. On your
own machine (`http://127.0.0.1:8765`) that is nothing at all and the tick is a
formality. On a café or conference network it is everyone else on that network.
Editing the address unticks the box — the acknowledgement is about one host,
not about the form.

**HTTPS with a certificate your system does not trust** — a self-signed
certificate, or a private CA. The app does not silently accept it and does not
silently refuse it. It probes the host, then shows an **Untrusted certificate**
sheet with the certificate's SHA-256 fingerprint as colon-grouped uppercase
hex. Compare it against what the hub's operator reads out:

```bash
# on the hub host, against the certificate the proxy serves
openssl x509 -fingerprint -sha256 -noout -in /path/to/cert.pem
# sha256 Fingerprint=88:44:DD:65:…

# or from anywhere, off the live handshake
openssl s_client -connect hub.example:8443 </dev/null 2>/dev/null \
  | openssl x509 -fingerprint -sha256 -noout
```

The sheet uses exactly that grouping so the two can be read group by group
instead of eyeballed as one 64-character run. If they match, **Add chamber anyway**
pins *that* certificate for *that* hub from then on. If they do not match,
someone else is answering for the hub — **Cancel** stores nothing.

A pinned hub that later presents a different certificate stops connecting
rather than quietly trusting the new one. When the operator legitimately
renews, remove the access in *Settings → Chamber access* and add it again to re-pin.

## Several chamber accesses at once

*Settings → Chamber access* lists every access link the app remembers. Each row
shows its label, hub address, Owner or Guest role, and `cryohub` version, with
**Add chamber** at the bottom and **Remove** on each row. Adding a second token
for the same hub keeps both scopes; it never replaces the chambers already saved.

The main list has **Owned** and **Joined** sections. If an owner token and an
invite token both expose the same chamber on the same hub, the app shows it
once under Owned so the admin controls remain available. Unread counts,
drafts, and read watermarks stay separate for every saved access.

An owner can use **Settings → Chambers → Copy admin link**. The app warns first
because anyone holding that link can administer every chamber on the selected
hub. The copied `cryochamber://` link opens the add-chamber form directly.

Hubs fail independently. A hub that stops answering has its rows' chips read
**· unreachable** and go muted — the row still shows the last thing that hub
said rather than disappearing — while every other hub keeps streaming, sending,
and updating. When it comes back, its rows recover on their own; no restart, no
re-entering the token.

Hub accounts live in the app's own private data directory rather than in
browser storage, so clearing a browser or losing a WebView's data does not cost
you your tokens. On macOS that file is
`~/Library/Application Support/com.cryochamber.console/hubs.json`. It holds
bearer tokens in the clear, protected by the file permissions on that
directory — treat it the way you treat `~/.config/cryo/cryohub-tokens.json`.

## Updating

There is no in-app updater and no update channel. Download the next release's
build and install it over the old one.

- **macOS:** replace *Cryochamber* in *Applications*. The hub store lives
  outside the bundle and is untouched. The right-click gesture may be needed
  again for the newly downloaded copy.
- **Android:** install the newer APK over the old one. The signing key stays the
  same, so Android treats it as an update and preserves the app's hub store.

The hubs themselves upgrade separately (`cargo install cryochamber`, then
`cryohub restart`). If a hub is older than the app, *Settings → Chamber access* says so
on that hub's row — *"hub is older — some features may be missing"*.
