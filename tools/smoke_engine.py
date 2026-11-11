import argparse
from collections import deque
import errno
import json
import math
import queue
import subprocess
import threading
import time

import chess


class UciProcess:
    def __init__(self, command, timeout):
        self.timeout = timeout
        self.lines = queue.Queue()
        self.history = deque(maxlen=40)
        self.process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.STDOUT, text=True, encoding="utf-8",
                                        errors="replace", bufsize=1)
        self.reader = threading.Thread(target=self.read_output, daemon=True)
        self.reader.start()

    def read_output(self):
        try:
            for line in self.process.stdout:
                self.lines.put(line.strip())
        finally:
            self.lines.put(None)

    def send(self, command):
        self.process.stdin.write(command + "\n")
        self.process.stdin.flush()

    def receive(self, deadline):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("UCI response deadline exceeded")
        try:
            line = self.lines.get(timeout=remaining)
        except queue.Empty as error:
            raise TimeoutError("UCI response deadline exceeded") from error
        if line is None:
            raise RuntimeError("Engine closed its output before the expected response")
        self.history.append(line)
        return line

    def wait_for(self, response):
        deadline = time.monotonic() + self.timeout
        lines = []
        while True:
            line = self.receive(deadline)
            lines.append(line)
            if line == response:
                return lines

    def close(self):
        if self.process.poll() is None:
            try:
                self.send("quit")
            except (BrokenPipeError, OSError):
                pass
            try:
                self.process.wait(timeout=min(1, self.timeout))
            except subprocess.TimeoutExpired:
                self.process.terminate()
                try:
                    self.process.wait(timeout=min(1, self.timeout))
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait(timeout=2)
        self.reader.join(timeout=1)
        try:
            self.process.stdin.close()
        except OSError as error:
            if self.process.poll() is None or error.errno not in (errno.EPIPE, errno.EINVAL):
                raise
        finally:
            if not self.reader.is_alive():
                self.process.stdout.close()


def legal_line(board, moves):
    position = board.copy(stack=False)
    if not moves:
        raise ValueError("Engine returned an empty PV")
    for token in moves:
        move = chess.Move.from_uci(token)
        if move not in position.legal_moves:
            raise ValueError(f"Engine returned an illegal move: {token}")
        position.push(move)


def smoke(command, version, timeout=30.0, depth=6, movetime=200):
    if not command or not version or not math.isfinite(timeout) or timeout <= 0 \
            or depth <= 0 or movetime <= 0:
        raise ValueError("Invalid engine smoke parameters")
    if chess.__version__ != "1.11.2":
        raise RuntimeError("Engine smoke requires chess==1.11.2")
    engine = UciProcess(command, timeout)
    results = []
    try:
        engine.send("uci")
        handshake = engine.wait_for("uciok")
        if f"id name Bitfox {version}" not in handshake:
            raise ValueError("Engine identity does not match the release version")
        for option in ("Hash", "Threads"):
            if not any(line.startswith(f"option name {option} type spin ") for line in handshake):
                raise ValueError(f"Engine is missing the {option} option")
        engine.send("setoption name Hash value 16")
        engine.send("isready")
        engine.wait_for("readyok")
        for threads in (1, 4):
            engine.send(f"setoption name Threads value {threads}")
            for limit, value, moves in (("depth", depth, []),
                                        ("movetime", movetime, ["e2e4", "c7c5", "g1f3"])):
                board = chess.Board()
                for move in moves:
                    board.push_uci(move)
                engine.send("ucinewgame")
                engine.send("position startpos" + (" moves " + " ".join(moves) if moves else ""))
                engine.send("isready")
                engine.wait_for("readyok")
                engine.send(f"go {limit} {value}")
                search_timeout = min(timeout, movetime / 1000 + 5) if limit == "movetime" else timeout
                deadline = time.monotonic() + search_timeout
                max_depth = max_nodes = 0
                has_pv = False
                while True:
                    tokens = engine.receive(deadline).split()
                    if not tokens:
                        continue
                    if tokens[0] == "info" and tokens[1:2] != ["string"]:
                        if "pv" in tokens:
                            legal_line(board, tokens[tokens.index("pv") + 1:])
                            has_pv = True
                        for field in ("depth", "nodes"):
                            if field in tokens:
                                measured = int(tokens[tokens.index(field) + 1])
                                if field == "depth":
                                    max_depth = max(max_depth, measured)
                                else:
                                    max_nodes = max(max_nodes, measured)
                    elif tokens[0] == "bestmove":
                        if len(tokens) not in (2, 4) or (len(tokens) == 4 and tokens[2] != "ponder"):
                            raise ValueError("Engine returned a malformed bestmove")
                        legal_line(board, [tokens[1]] + (tokens[3:] if len(tokens) == 4 else []))
                        if not has_pv or max_depth < 1 or max_nodes < 1:
                            raise ValueError("Engine returned bestmove without searched nodes and a legal PV")
                        if limit == "depth" and max_depth < value:
                            raise ValueError("Engine stopped before the requested depth")
                        results.append({"threads": threads, "limit": f"{limit} {value}",
                                        "depth": max_depth, "nodes": max_nodes, "bestmove": tokens[1]})
                        break
        engine.send("quit")
        try:
            code = engine.process.wait(timeout=min(5, timeout))
        except subprocess.TimeoutExpired as error:
            raise TimeoutError("Engine did not exit after quit") from error
        if code != 0:
            raise RuntimeError(f"Engine exited with status {code}")
        return results
    except (ValueError, RuntimeError, TimeoutError, OSError, IndexError) as error:
        raise RuntimeError(f"Engine smoke failed: {error}\n" + "\n".join(engine.history)) from error
    finally:
        engine.close()


def main():
    parser = argparse.ArgumentParser(description="Verify packaged Bitfox UCI searches and shutdown.")
    parser.add_argument("--version", required=True)
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    for result in smoke(command, args.version, timeout=args.timeout):
        print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
