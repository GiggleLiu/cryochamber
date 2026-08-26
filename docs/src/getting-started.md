# Getting started

This page walks one machine from nothing to a running hub, with a first
chamber you can read on your phone and share with a friend. Five steps: start
the hub, set up the Pi agent, connect, create the first chamber, share it.

> **Platform:** the hub runs on macOS and Linux. The native app runs on
> Apple Silicon macOS; on a phone, the browser console works as an
> installable PWA.

## 1. Start the hub

```bash
cargo install cryochamber
cryohub start
```

`cryohub start` installs the hub as a user service (it survives reboots),
binds `http://127.0.0.1:8765`, and on the first run prints the **owner
token**:

```
Owner token (save it — or reprint later with `cryohub token owner`):
3f9c…
```

That token is your login — there are no accounts, passwords, or e-mail.
`cryohub token owner` reprints it any time.

## 2. Set up the Pi agent

A chamber needs an agent runner installed on the hub host. **Pi** is the
built-in default:

```bash
npm install -g @mariozechner/pi-coding-agent
which pi        # the hub verifies this executable exists before starting a chamber
```

Pi reads provider API keys from the environment, and you do not have to
export anything globally: the **+ New chamber** sheet (next step) has a
folded *API key* section that writes the key into that chamber's own
`cryo.toml` as `[provider] env`, and the daemon injects it into every
session.

Prefer a different runner? `claude`, `opencode`, `codex`, and `kimi` are in
the *Settings → Default agent* dropdown — see [choosing which agent a chamber
runs](./agent-console.md#choosing-which-agent-a-chamber-runs).

## 3. Connect

**On the hub machine itself:** open `http://127.0.0.1:8765` in a browser and
paste the owner token.

**From a phone or another machine:** the hub stays bound to loopback, so put
a TLS-terminating reverse proxy in front of it — the [public deployment
section](./agent-console.md#public-deployment-phone-outside-your-network) is
a complete Caddy recipe. Then either:

- open `https://agents.example.com` in the phone's browser and *Add to Home
  Screen*, or
- on a Mac, install the [native app](./install-app.md) and add the hub —
  address plus owner token. The app can hold **several hubs at once** in one
  merged list; a browser install is bound to the one hub that served it.

## 4. A first chamber: the host manager

A good bootstrap chamber is one that looks after the hub host itself. In the
console, tap **+ New chamber**, name it `host-manager`, paste your API key
into the folded *API key* section, and create it — the sheet scaffolds the
chamber and starts it in one action. Then open **⋯ Chamber controls → Plan →
Edit plan** and give it a brief like:

```markdown
# Host manager

You look after this machine. Once a day:

- Check free disk space (`df -h /`); warn me when usage crosses 85%.
- Check that `cryohub status` and the other chambers' `cryo status`
  look healthy.
- Report in a few lines; only raise what needs a human.

Wake once a day around 09:00. If I send you a message, handle it and
answer.
```

Nothing needs a restart after editing the plan: the agent reads `plan.md` at
the top of every session. From here it schedules its own wakes, reports every
session into the chamber's conversation, and answers anything you send it.

## 5. Share a chamber with a friend

Open the chamber, tap **Invite** in its header, optionally type the friend's
name, and **Copy invite link**. Send it to them:

- opened in a browser, the link *is* the sign-in — they land directly in
  that one chamber;
- pasted into the app's *Add a hub → Invite link* field, it fills the
  address and token for them.

The link is scoped to that single chamber: a guest can read and send there,
and never sees your other chambers or any controls. **People with access**
on the same sheet lists every active link; **Remove** revokes one instantly.
Sharing requires that the friend can reach the hub — that is step 3's
reverse proxy.
