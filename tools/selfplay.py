
import argparse
import glob
import json
import math
import os
import random
import select
import subprocess
import sys
import time

import chess

OPENINGS = [
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq c6 0 2",
    "rnbqkbnr/ppp1pppp/8/3p4/3P4/8/PPP1PPPP/RNBQKBNR w KQkq d6 0 2",
    "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2",
    "rnbqkb1r/pppppppp/5n2/8/2P5/8/PP1PPPPP/RNBQKBNR w KQkq - 1 2",
    "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
    "rnbqkb1r/pppp1ppp/5n2/4p3/2P5/8/PP1PPPPP/RNBQKBNR w KQkq - 2 3",
    "rnbqkbnr/pp2pppp/8/2pp4/3P4/2N5/PPP1PPPP/R1BQKBNR w KQkq - 0 3",
    "r1bqkbnr/pp1ppppp/2n5/2p5/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
    "rnbqkbnr/ppp2ppp/8/3pp3/4P3/3P4/PPP2PPP/RNBQKBNR w KQkq - 0 3",
    "rnbqkb1r/pppp1ppp/5n2/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 4 3",
    "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4",
]


def load_openings(limit, seed):
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "data", "openings")
    pool = []
    for path in sorted(glob.glob(os.path.join(root, "eco*.json"))):
        try:
            with open(path) as fh:
                book = json.load(fh)
        except (OSError, ValueError):
            continue
        if isinstance(book, dict):
            entries = book.keys()
        elif isinstance(book, list):
            entries = book
        else:
            continue
        for fen in entries:
            if not isinstance(fen, str):
                continue
            parts = fen.split()
            if len(parts) == 6 and parts[4].isdigit() and parts[5].isdigit():
                if 2 <= int(parts[5]) <= 8 and int(parts[4]) <= 4:
                    pool.append(fen)
    if not pool:
        return OPENINGS
    random.Random(seed).shuffle(pool)
    out = []
    for fen in pool:
        try:
            chess.Board(fen)
        except ValueError:
            continue
        out.append(fen)
        if len(out) >= limit:
            break
    return out or OPENINGS


class Engine:
    def __init__(self, path):
        self.proc = subprocess.Popen(
            [path],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            bufsize=0,
        )
        self.buffer = b""
        self._send("uci")
        self._wait("uciok")
        self.ready()

    def _send(self, line):
        self.proc.stdin.write((line + "\n").encode())
        self.proc.stdin.flush()

    def _readline(self, timeout=None):
        deadline = None if timeout is None else time.monotonic() + timeout
        while b"\n" not in self.buffer:
            wait = None if deadline is None else max(0.0, deadline - time.monotonic())
            ready, _, _ = select.select([self.proc.stdout], [], [], wait)
            if not ready:
                return None
            chunk = os.read(self.proc.stdout.fileno(), 4096)
            if not chunk:
                return None
            self.buffer += chunk
        line, self.buffer = self.buffer.split(b"\n", 1)
        return line.decode(errors="replace")

    def _wait(self, token, timeout=120.0):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            line = self._readline(deadline - time.monotonic())
            if line is None:
                break
            if line.strip().startswith(token):
                return
        raise TimeoutError(f"engine did not return {token}")

    def ready(self):
        self._send("isready")
        self._wait("readyok")

    def new_game(self):
        self._send("ucinewgame")
        self.ready()

    def bestmove(self, fen, nodes=None, movetime=None, timeout=120.0):
        self._send(f"position fen {fen}")
        if movetime is not None:
            self._send(f"go movetime {movetime}")
        else:
            self._send(f"go nodes {nodes}")
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            line = self._readline(deadline - time.monotonic())
            if line is None:
                break
            if line.startswith("bestmove"):
                return line.split()[1]
        return None

    def close(self):
        try:
            self._send("quit")
            self.proc.wait(timeout=5)
        except Exception:
            self.proc.kill()


def play_game(white, black, fen, nodes, movetime):
    board = chess.Board(fen)
    white.new_game()
    black.new_game()
    while not board.is_game_over(claim_draw=True):
        engine = white if board.turn == chess.WHITE else black
        uci = engine.bestmove(board.fen(), nodes, movetime)
        if uci is None or uci == "0000":
            break
        try:
            move = chess.Move.from_uci(uci)
        except ValueError:
            return chess.BLACK if board.turn == chess.WHITE else chess.WHITE
        if move not in board.legal_moves:
            return chess.BLACK if board.turn == chess.WHITE else chess.WHITE
        board.push(move)
    outcome = board.outcome(claim_draw=True)
    if outcome is None or outcome.winner is None:
        return None
    return outcome.winner


def elo_diff(score):
    if score <= 0:
        return -800.0
    if score >= 1:
        return 800.0
    return -400.0 * math.log10(1.0 / score - 1.0)


def error_margin(wins, losses, draws):
    n = wins + losses + draws
    if n == 0:
        return 0.0
    score = (wins + 0.5 * draws) / n
    var = (wins * (1 - score) ** 2 + losses * (0 - score) ** 2 + draws * (0.5 - score) ** 2) / n
    stddev = math.sqrt(var / n)
    return (elo_diff(min(score + stddev, 0.999)) - elo_diff(max(score - stddev, 0.001))) / 2.0


def los(wins, losses):
    if wins + losses == 0:
        return 50.0
    return 50.0 * (1.0 + math.erf((wins - losses) / math.sqrt(2.0 * (wins + losses))))


def sprt_llr(wins, losses, draws, elo0, elo1):
    n = wins + losses + draws
    if n == 0 or wins == 0 or losses == 0:
        return 0.0
    score = (wins + 0.5 * draws) / n
    var = (wins * (1 - score) ** 2 + losses * score ** 2 + draws * (0.5 - score) ** 2) / n
    if var == 0:
        return 0.0
    s0 = 1.0 / (1.0 + 10.0 ** (-elo0 / 400.0))
    s1 = 1.0 / (1.0 + 10.0 ** (-elo1 / 400.0))
    return (s1 - s0) * (2 * score - s0 - s1) / (2.0 * var) * n


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("baseline")
    ap.add_argument("candidate")
    ap.add_argument("--games", type=int, default=200)
    ap.add_argument("--nodes", type=int, default=20000)
    ap.add_argument("--movetime", type=int)
    ap.add_argument("--elo0", type=float, default=0.0)
    ap.add_argument("--elo1", type=float, default=5.0)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--openings-limit", type=int, default=2000)
    ap.add_argument("--builtin-openings", action="store_true")
    args = ap.parse_args()

    for path in (args.baseline, args.candidate):
        if not os.path.isfile(path) or not os.access(path, os.X_OK):
            sys.exit(f"error: not an executable engine binary: {path}")

    openings = OPENINGS if args.builtin_openings else load_openings(args.openings_limit, args.seed)
    if not openings:
        openings = OPENINGS

    base = Engine(args.baseline)
    cand = Engine(args.candidate)

    wins = losses = draws = 0
    pairs = (args.games + 1) // 2
    lower = math.log(0.05 / 0.95)
    upper = math.log(0.95 / 0.05)

    for i in range(pairs):
        fen = openings[i % len(openings)]
        for cand_is_white in (True, False):
            white, black = (cand, base) if cand_is_white else (base, cand)
            winner = play_game(white, black, fen, args.nodes, args.movetime)
            if winner is None:
                draws += 1
            elif (winner == chess.WHITE) == cand_is_white:
                wins += 1
            else:
                losses += 1

            n = wins + losses + draws
            score = (wins + 0.5 * draws) / n
            llr = sprt_llr(wins, losses, draws, args.elo0, args.elo1)
            print(
                f"\r{n:4d} games  +{wins} ={draws} -{losses}  "
                f"{elo_diff(score):+7.1f} +/-{error_margin(wins, losses, draws):5.1f} Elo  "
                f"LOS {los(wins, losses):5.1f}%  LLR {llr:+5.2f}",
                end="",
                flush=True,
            )
            if llr <= lower or llr >= upper:
                break
        else:
            continue
        break

    print()
    verdict = "PASS (candidate stronger)" if sprt_llr(wins, losses, draws, args.elo0, args.elo1) >= upper else (
        "FAIL (not stronger)" if sprt_llr(wins, losses, draws, args.elo0, args.elo1) <= lower else "INCONCLUSIVE"
    )
    print(f"verdict: {verdict}")
    base.close()
    cand.close()
    sys.exit(0)


if __name__ == "__main__":
    main()
