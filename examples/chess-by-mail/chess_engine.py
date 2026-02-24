#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = ["python-chess"]
# ///
"""Chess engine helper for the cryochamber chess-by-mail example.

Run with: uv run chess_engine.py <command> [args...]

Usage:
    uv run chess_engine.py board [FEN]           # Print ASCII board
    uv run chess_engine.py move FEN MOVE         # Apply move, print new FEN + board
    uv run chess_engine.py legal FEN             # List all legal moves
    uv run chess_engine.py suggest FEN [N]       # Suggest N best moves (default: 3)
    uv run chess_engine.py status FEN            # Game status (check/checkmate/stalemate/draw)
    uv run chess_engine.py parse FEN INPUT       # Parse human input to UCI (e2e4, Nf3, O-O, etc.)

Exit codes:
    0 = success
    1 = illegal move or invalid input
    2 = game over (checkmate/stalemate)

Requires: pip install python-chess
"""

import sys

import chess


UNICODE_PIECES = {
    "R": "\u2656", "N": "\u2658", "B": "\u2657", "Q": "\u2655", "K": "\u2654", "P": "\u2659",
    "r": "\u265c", "n": "\u265e", "b": "\u265d", "q": "\u265b", "k": "\u265a", "p": "\u265f",
}


def print_board(board: chess.Board) -> None:
    """Print the board with rank/file labels using Unicode chess pieces."""
    print()
    for rank in range(7, -1, -1):
        pieces = []
        for file in range(8):
            piece = board.piece_at(chess.square(file, rank))
            pieces.append(UNICODE_PIECES[piece.symbol()] if piece else "\u00b7")
        print(f"  {rank + 1}  {' '.join(pieces)}")
    print()
    print("     a b c d e f g h")
    print()
    print(f"  FEN: {board.fen()}")
    print(f"  Turn: {'white' if board.turn == chess.WHITE else 'black'}")
    if board.is_check():
        print("  ** CHECK **")


def parse_move(board: chess.Board, move_str: str) -> chess.Move:
    """Parse a move string in any common format."""
    move_str = move_str.strip()

    # Try UCI format (e2e4)
    try:
        move = chess.Move.from_uci(move_str)
        if move in board.legal_moves:
            return move
    except (chess.InvalidMoveError, ValueError):
        pass

    # Try SAN format (Nf3, e4, O-O, etc.)
    try:
        return board.parse_san(move_str)
    except (chess.InvalidMoveError, chess.IllegalMoveError, chess.AmbiguousMoveError, ValueError):
        pass

    # Try with promotion (e7e8q)
    if len(move_str) == 5:
        try:
            move = chess.Move.from_uci(move_str)
            if move in board.legal_moves:
                return move
        except (chess.InvalidMoveError, ValueError):
            pass

    raise ValueError(f"Cannot parse move: {move_str}")


def game_status(board: chess.Board) -> str:
    """Return the game status string."""
    if board.is_checkmate():
        winner = "black" if board.turn == chess.WHITE else "white"
        return f"checkmate — {winner} wins"
    if board.is_stalemate():
        return "stalemate — draw"
    if board.is_insufficient_material():
        return "draw — insufficient material"
    if board.is_fifty_moves():
        return "draw — fifty-move rule"
    if board.is_repetition():
        return "draw — threefold repetition"
    if board.is_check():
        return "check"
    return "in progress"


def suggest_moves(board: chess.Board, n: int = 3) -> list[tuple[str, str]]:
    """Suggest moves with simple heuristics. Returns (san, reason) pairs.

    This uses basic chess heuristics, not a full engine:
    - Captures scored by material gain (MVV-LVA)
    - Checks get a bonus
    - Center control gets a bonus
    - Castling gets a bonus
    """
    piece_values = {
        chess.PAWN: 1, chess.KNIGHT: 3, chess.BISHOP: 3,
        chess.ROOK: 5, chess.QUEEN: 9, chess.KING: 0,
    }
    center = {chess.E4, chess.D4, chess.E5, chess.D5}
    extended_center = {chess.C3, chess.D3, chess.E3, chess.F3,
                       chess.C4, chess.C5, chess.C6,
                       chess.F4, chess.F5, chess.F6,
                       chess.D6, chess.E6}

    scored = []
    for move in board.legal_moves:
        score = 0.0
        reasons = []

        # Capture value
        if board.is_capture(move):
            victim = board.piece_at(move.to_square)
            attacker = board.piece_at(move.from_square)
            if victim and attacker:
                gain = piece_values.get(victim.piece_type, 0) - piece_values.get(attacker.piece_type, 0) * 0.1
                score += gain + 5
                reasons.append(f"captures {chess.piece_name(victim.piece_type)}")

        # Check bonus
        board.push(move)
        if board.is_check():
            score += 3
            reasons.append("gives check")
        if board.is_checkmate():
            score += 100
            reasons = ["checkmate!"]
        board.pop()

        # Castling
        if board.is_castling(move):
            score += 4
            reasons.append("castles for king safety")

        # Center control
        if move.to_square in center:
            score += 2
            reasons.append("controls center")
        elif move.to_square in extended_center:
            score += 1

        # Development (knights and bishops off back rank)
        piece = board.piece_at(move.from_square)
        if piece and piece.piece_type in (chess.KNIGHT, chess.BISHOP):
            from_rank = chess.square_rank(move.from_square)
            if (piece.color == chess.WHITE and from_rank == 0) or \
               (piece.color == chess.BLACK and from_rank == 7):
                score += 2
                if "controls center" not in reasons:
                    reasons.append("develops piece")

        if not reasons:
            reasons.append("positional")

        scored.append((move, score, "; ".join(reasons)))

    scored.sort(key=lambda x: -x[1])
    result = []
    for move, _score, reason in scored[:n]:
        san = board.san(move)
        result.append((san, reason))
    return result


def cmd_board(args: list[str]) -> None:
    fen = args[0] if args else chess.STARTING_FEN
    board = chess.Board(fen)
    print_board(board)


def cmd_move(args: list[str]) -> None:
    if len(args) < 2:
        print("Usage: chess_engine.py move FEN MOVE", file=sys.stderr)
        sys.exit(1)
    fen, move_str = args[0], args[1]
    board = chess.Board(fen)

    try:
        move = parse_move(board, move_str)
    except ValueError as e:
        print(f"Error: {e}", file=sys.stderr)
        print(f"Legal moves: {', '.join(board.san(m) for m in board.legal_moves)}", file=sys.stderr)
        sys.exit(1)

    san = board.san(move)
    board.push(move)
    print(f"Move: {san}")
    print_board(board)

    status = game_status(board)
    print(f"  Status: {status}")
    if "checkmate" in status or "stalemate" in status or "draw" in status:
        sys.exit(2)


def cmd_legal(args: list[str]) -> None:
    if not args:
        print("Usage: chess_engine.py legal FEN", file=sys.stderr)
        sys.exit(1)
    board = chess.Board(args[0])
    moves = [board.san(m) for m in board.legal_moves]
    print(f"Legal moves ({len(moves)}): {', '.join(moves)}")


def cmd_suggest(args: list[str]) -> None:
    if not args:
        print("Usage: chess_engine.py suggest FEN [N]", file=sys.stderr)
        sys.exit(1)
    board = chess.Board(args[0])
    n = int(args[1]) if len(args) > 1 else 3
    suggestions = suggest_moves(board, n)
    print(f"Suggested moves for {'white' if board.turn == chess.WHITE else 'black'}:")
    for i, (san, reason) in enumerate(suggestions, 1):
        print(f"  {i}. {san} — {reason}")


def cmd_status(args: list[str]) -> None:
    if not args:
        print("Usage: chess_engine.py status FEN", file=sys.stderr)
        sys.exit(1)
    board = chess.Board(args[0])
    print_board(board)
    status = game_status(board)
    print(f"  Status: {status}")
    move_num = board.fullmove_number
    print(f"  Move: {move_num}")
    if "checkmate" in status or "stalemate" in status or "draw" in status:
        sys.exit(2)


def cmd_parse(args: list[str]) -> None:
    if len(args) < 2:
        print("Usage: chess_engine.py parse FEN INPUT", file=sys.stderr)
        sys.exit(1)
    board = chess.Board(args[0])
    try:
        move = parse_move(board, args[1])
        print(f"UCI: {move.uci()}")
        print(f"SAN: {board.san(move)}")
    except ValueError as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)


def main() -> None:
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(0)

    commands = {
        "board": cmd_board,
        "move": cmd_move,
        "legal": cmd_legal,
        "suggest": cmd_suggest,
        "status": cmd_status,
        "parse": cmd_parse,
    }

    cmd = sys.argv[1]
    if cmd in ("-h", "--help"):
        print(__doc__)
        sys.exit(0)

    if cmd not in commands:
        print(f"Unknown command: {cmd}", file=sys.stderr)
        print(f"Available: {', '.join(commands)}", file=sys.stderr)
        sys.exit(1)

    commands[cmd](sys.argv[2:])


if __name__ == "__main__":
    main()
