# Chess by Mail

## Goal

Play a correspondence chess game against a human opponent. Maintain the board state across sessions, respond to moves received via inbox messages, and adapt your checking schedule based on activity.

## Closing ritual — every session

Every session of this plan ends with the standard two-call ritual (see `CLAUDE.md` / `AGENTS.md`):

```
cryo-agent todo add "<next step>" --at <WAKE_TIME>
cryo-agent hibernate --summary "<what I did>"
```

Wake times come **only** from TODOs. `hibernate` does not schedule anything. The sole exception is game-over, where you use `hibernate --complete` and no TODO.

If you forget the TODO, the chamber goes silent. If you forget `hibernate`, the daemon retries. Do both, in order, at the end of every path below.

## Chess Engine

Use `chess_engine.py` for all board operations. It handles move validation, board display, and move suggestions. Run it with `uv run chess_engine.py` (uv auto-installs the dependency).

Commands:

```bash
uv run chess_engine.py board [FEN]           # Print ASCII board (default: starting position)
uv run chess_engine.py move FEN MOVE         # Apply move, print new FEN + board + status
uv run chess_engine.py legal FEN             # List all legal moves
uv run chess_engine.py suggest FEN [N]       # Suggest N best moves with explanations
uv run chess_engine.py status FEN            # Game status (check/checkmate/stalemate/draw)
uv run chess_engine.py parse FEN INPUT       # Parse human input to UCI/SAN
```

Exit codes: 0 = success, 1 = illegal move, 2 = game over.

## Session Decision Tree

Every wake, pick exactly one branch based on state.

### Branch A — First session (no game in progress)

State: `NOTES.md` has no FEN, or this is session #1.

1. Run `uv run chess_engine.py board` to print the starting position.
2. Run `uv run chess_engine.py suggest FEN 3` for 3 opening-move suggestions with explanations.
3. `cryo-agent send "<board + suggestions + instructions for reply>"`.
4. Save the starting FEN and an empty move history to `NOTES.md`.
5. **Closing ritual:** human may take hours to reply — schedule a default check-in.
   ```
   cryo-agent todo add "Check inbox for white's opening move" --at $(cryo-agent time "+30 minutes")
   cryo-agent hibernate --summary "Sent opening board; waiting for white's first move."
   ```

### Branch B — Inbox has new move(s) from the human

State: one or more messages in `messages/inbox/` that you have not yet processed.

1. For each move, in order received:
   a. Parse with `uv run chess_engine.py parse FEN INPUT` if the move isn't already in UCI form.
   b. Apply with `uv run chess_engine.py move FEN MOVE` — update FEN.
   c. If game over (exit code 2) → go to Branch D.
   d. Otherwise compute your response: `uv run chess_engine.py suggest FEN` to pick a candidate, then `uv run chess_engine.py move FEN YOUR_MOVE`.
   e. If your move ends the game (exit code 2) → go to Branch D.
   f. Otherwise run `uv run chess_engine.py suggest FEN 3` for 3 suggestions for the human.
   g. `cryo-agent send "<updated board + your move + 3 suggestions>"`.
2. Update `NOTES.md` with new FEN, move history, and how long the human took to reply (for adaptive timing).
3. Mark the inbox-check TODO(s) done: `cryo-agent todo done <id>`.
4. **Closing ritual:** schedule the next inbox check based on the human's pace.
   ```
   cryo-agent todo add "Check inbox for white's next move" --at <ADAPTIVE_WAKE>
   cryo-agent hibernate --summary "Processed move(s); waiting for white."
   ```

**Adaptive timing for the next-wake TODO:**
- If the human replies within seconds → next check in ~1 min.
- Within minutes → ~5 min.
- Within hours → ~30 min.
- Within a day → ~4 hours.
- Clamp: minimum 1 min, maximum 1 day.
- Unsure → default to 30 min.

### Branch C — No new moves (just a scheduled wake)

State: inbox empty, game in progress, you woke because of your own TODO.

1. Confirm inbox is empty: `cryo-agent receive`.
2. Mark the triggering TODO done: `cryo-agent todo done <id>`.
3. **Closing ritual:** back off slightly (human is still thinking).
   ```
   cryo-agent todo add "Check inbox for white's next move" --at <NEXT_CHECK>
   cryo-agent hibernate --summary "No reply yet; rechecking at <NEXT_CHECK>."
   ```

   Back-off guidance: if this is the Nth consecutive empty check, roughly double the interval (clamped to 1 day). Don't spam the inbox.

### Branch D — Game over (checkmate, stalemate, draw, or resignation)

State: `uv run chess_engine.py move` exited 2, or the human sent "resign" / "draw".

1. `cryo-agent send "<final board + result announcement + thanks for the game>"`.
2. Append the final result to `NOTES.md`.
3. **Terminal hibernate — no TODO this time:**
   ```
   cryo-agent hibernate --complete --summary "Game over: <result>"
   ```

## Configuration

- AI plays: black
- Notation: accept both algebraic (e4, Nf3, O-O) and coordinate (e2e4)

## Notes

- Store the board as a FEN string in `NOTES.md` so you can reconstruct it on wake.
- Store the full move history (e.g., `1. e4 e5 2. Nf3`) in `NOTES.md`.
- If the human sends multiple moves at once, process them in order and respond to each, but only send one consolidated `cryo-agent send` at the end (avoid spamming).
- Use `cryo-agent time "+10 minutes"` to compute wake times — never hand-write timestamps.
