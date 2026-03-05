# Chess by Mail

Play correspondence chess against an AI agent, powered by cryochamber.

The AI polls for your moves on a configurable interval. If you're away too long, it goes to sleep — send a move and wake it when you're ready to continue.

## Why Cryochamber

A cron job can't do this because:
- The AI decides when to stop checking (adaptive patience, not fixed schedule)
- Wake-up from deep sleep is event-driven (your next move), not time-driven
- Board state and strategy notes persist across arbitrarily long gaps
- Multiple moves can accumulate; the AI processes them all on wake

## Prerequisites

- [uv](https://docs.astral.sh/uv/) (the chess engine script uses uv for dependency management)

## Quick Start

```bash
cd examples/chess-by-mail
cryo init && cryo start
cryo web   # open the browser chat UI (port 3947)
```

## Playing

```bash
# Send a move (algebraic or coordinate notation)
cryo send "e2e4"
cryo send "Nf3" --wake  # wake the AI immediately

# Or use the web UI
cryo web
```

## How It Works

The AI uses `chess_engine.py` (powered by `python-chess` via uv) for all chess operations. After each move, the AI recommends 3 candidate moves for you with tactical explanations.

## Playing via Zulip

You can play from the Zulip web UI instead of the terminal by connecting a Zulip stream. See [Zulip Sync](../zulip-sync.md) for full setup details.

```bash
cd examples/chess-by-mail
cryo-zulip init --config ~/.zuliprc --stream chess-game
cryo init && cryo start
cryo-zulip sync --interval 30
```

Now send your moves as messages in the Zulip stream. The sync daemon delivers them to the agent's inbox and posts replies back to the stream.

To stop: `cryo cancel && cryo-zulip unsync`

## Configuration

Edit `plan.md` to change:
- Which color the AI plays (default: black)
- Check interval (default: 10 minutes; set to `1 minute` for a fast demo)
- Patience threshold (default: 5 checks before sleeping)
