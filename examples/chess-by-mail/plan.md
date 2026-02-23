# Chess by Mail

## Goal

Play a correspondence chess game against a human opponent. Maintain the board state across sessions, respond to moves received via inbox messages, and adapt your checking schedule based on activity.

## Tasks

1. Initialize the board. Print the starting position in ASCII. If playing white, make your opening move.
2. Each session: check inbox for human moves. Validate each move, apply it to the board, and compute your response. Print the updated ASCII board.
3. If no move is received, increment your idle counter. After the patience threshold, announce you are going to sleep (omit the WAKE marker).
4. On wake from deep sleep: check inbox for the move that triggered the wake, respond, and resume normal polling.
5. Detect checkmate, stalemate, draw, or resignation. Announce the result and end the plan (omit WAKE marker).

## Configuration

- AI plays: black
- Check interval: 10 minutes
- Patience: 5 checks (go to sleep after 5 empty checks)
- Notation: accept both algebraic (e4, Nf3, O-O) and coordinate (e2e4)

## Notes

- Store the board as a FEN string in your PLAN marker so you can reconstruct it on wake.
- Store the full move history (e.g., `1. e4 e5 2. Nf3`) in PLAN as well.
- If the human sends multiple moves at once, process them in order and respond to each.
- Use `make time OFFSET="+10 minutes"` to compute your next WAKE time.
- When going to deep sleep, do NOT emit a WAKE marker. The human will run `cryo wake` manually.
