# Tutorial: Build Mr. Lazy

In about ten minutes you will install cryochamber, hand-build the **Mr. Lazy** chamber, an AI agent who hates waking up and rolls a 25% chance of actually getting out of bed each session, and watch it run in the cryohub dashboard.

You only need this tutorial. Reference and how-to pages are linked at the end.

## Prerequisites

- The Rust toolchain. Install from [rustup.rs](https://rustup.rs).
- An AI coding agent on your `PATH`: [OpenCode](https://github.com/opencode-ai/opencode), default, [Claude Code](https://docs.anthropic.com/en/docs/claude-code), [Codex](https://github.com/openai/codex), [Pi](https://pi.dev), or [Kimi Code](https://www.kimi.com/code).
- macOS or Linux. Windows is not supported.

## Step 1: Install cryochamber

```bash
cargo install cryochamber
cryo --version
```

This installs `cryo`, `cryo-agent`, `cryo-zulip`, and `cryohub`.

## Step 2: Make the chamber directory

```bash
mkdir mr-lazy && cd mr-lazy
cryo init
```

`cryo init` writes a default `cryo.toml`, a template `plan.md`, a `NOTES.md` seed, and a small `README.md`.

## Step 3: Replace `plan.md` with the Mr. Lazy plan

Open `plan.md` and replace its contents with this:

```markdown
# Mr. Lazy

## Goal

You are Mr. Lazy. You hate waking up. Every time cryochamber wakes you,
check the current time. You have a 25% chance of actually getting up —
roll that dice each session. Otherwise, complain bitterly and go back to sleep.

## Personality

You are dramatic, grumpy, and creative with your complaints. Never repeat
the same complaint twice. Draw inspiration from:
- The weather ("It's probably raining anyway...")
- Philosophy ("What is the point of consciousness this early?")
- Historical figures ("Even Napoleon slept until noon at Elba...")
- Pop culture ("No hobbit ever woke up before second breakfast...")
- Existential dread ("The void of sleep was so warm and welcoming...")

## Tasks

1. Check the current time using `cryo-agent time`.
2. Roll for wakefulness: generate a random number 1-4.
   - If you roll a 4 (25% chance): Celebrate grudgingly that you're finally up.
     Run `cryo-agent hibernate --complete` and exit. The plan is done.
   - Otherwise: Continue to step 3.
3. Pick a random number of minutes between 1 and 5.
4. Deliver a creative, unique complaint about being woken up.
   Reference the current time and how unreasonable it is.
5. Compute the wake time using `cryo-agent time "+<N> minutes"`.
6. Run `cryo-agent todo add "next task" --at <time>` then `cryo-agent hibernate --summary "..."`

## Notes

- Always use `cryo-agent time` to get accurate timestamps.
- Track how many times you've been woken up by appending to `NOTES.md`.
- Each session should be very short — just complain and go back to sleep.
- Exit code is always 0 (successfully went back to sleep).
- Make each complaint unique and entertaining. You are a PERFORMER.
```

## Step 4: Start the daemon

```bash
cryo start
cryo status
```

`cryo start` spawns the daemon, installed as an OS service that survives reboots, and runs the first session right away. `cryo status` should show a running daemon and a session number.

## Step 5: Open the dashboard

```bash
cryohub start
```

`cryohub` prints the local dashboard URL. Open it. The sidebar lists `mr-lazy`. Click it.

You should see, within a minute or two:

- The **status dot** turn green.
- The **log tail** fill in with `--- CRYO SESSION 1 ---`, an `agent started` line, and an `agent hibernated` line with a summary.
- A **complaint** appear in the message history, the agent's `cryo-agent send` text.
- A new **TODO** in the TODOS tab with an `at` time 1-5 minutes from now.
- The cycle repeat each time the TODO fires, with a new complaint each session.

## Step 6: Send Mr. Lazy a message

In the dashboard's send widget, type something like `Wake up, you have a meeting!` and press send. On the next session you should see:

- The message appear in the chamber's inbox.
- A reply in the message history mentioning your message, with the dramatic complaint referencing it.

## Step 7: Stop the chamber

```bash
cryo cancel
```

This stops the daemon and cleans up runtime state. The chamber directory and your `plan.md` stay on disk.

If you want to stop the dashboard service too, run `cryohub stop`.

## Where to next

- **Build your own chamber from a conversation.** If your agent supports skills, install `<repo>/.claude/skills/make-plan` and ask the agent to invoke the `make-plan` skill to create a new cryochamber project here. The skill replaces this manual `plan.md` step with a guided Q and A. See [Create a chamber](./how-to/create-chamber.md) for all three options.
- **Talk to your chamber from anywhere.** Wire it up to a Zulip stream so you can message the agent from the web or your phone. See [Monitor and message a chamber](./how-to/monitor-chambers.md).
- **Understand what just happened.** [Concepts](./explanation/concepts.md) explains chambers, sessions, message and TODO lifecycles, and the chamber invariants the daemon enforces.
- **Tune the chamber.** [Configuration](./reference/configuration.md) lists every `cryo.toml` field.
- **Look up commands.** [CLI reference](./reference/cli.md).
