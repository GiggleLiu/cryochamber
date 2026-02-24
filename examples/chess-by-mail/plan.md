# Chess by Mail

## Goal

Play a correspondence chess game against a human opponent. Maintain the board state across sessions, respond to moves received via inbox messages, and adapt your checking schedule based on activity.

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

## Tasks

1. Initialize the board. Run `uv run chess_engine.py board` to print the starting position. Run `uv run chess_engine.py suggest FEN 3` to recommend 3 moves for the human (white) with explanations. Send the board and suggestions via `cryo-agent send`.
2. Each session: check inbox for human moves. For each move:
   a. Validate and apply with `uv run chess_engine.py move FEN MOVE`.
   b. If the game is not over, compute your response move using `uv run chess_engine.py suggest FEN` to evaluate candidates, pick one, and apply it with `uv run chess_engine.py move FEN YOUR_MOVE`.
   c. After your move, run `uv run chess_engine.py suggest FEN 3` to recommend 3 moves for the human with explanations of each (tactical, positional, defensive, etc.).
   d. Send the updated board, your move, and the human's suggested moves via `cryo-agent send`.
3. If no move is received, hibernate and wake again in 2–10 minutes. **Never stop checking.** The human may take minutes, hours, or days to respond — that is normal for correspondence chess. Always hibernate with a wake time; never use `--complete` unless the game is over.
4. Detect checkmate, stalemate, draw, or resignation (exit code 2 from chess_engine.py). This is the **only** condition that ends the session. Announce the result and run `cryo-agent hibernate --complete`.

## Configuration

- AI plays: black
- Check interval: 2–10 minutes (shorter when the game is active, longer when idle)
- Notation: accept both algebraic (e4, Nf3, O-O) and coordinate (e2e4)

## Notes

- Store the board as a FEN string in your `cryo-agent note` so you can reconstruct it on wake.
- Store the full move history (e.g., `1. e4 e5 2. Nf3`) in notes as well.
- If the human sends multiple moves at once, process them in order and respond to each.
- Use `cryo-agent time "+10 minutes"` to compute your next wake time.
- Use `cryo-agent send` to send your moves and commentary to the human.
