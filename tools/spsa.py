import argparse
import json
import os
import random
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import chess

from selfplay import Engine, load_openings, play_game

EXCLUDED = {"hash", "threads"}


def discover_params(engine):
    options = engine.list_spin_options()
    return [o for o in options if o["name"].lower() not in EXCLUDED]


def load_theta(path, params):
    theta = {p["name"]: float(p["default"]) for p in params}
    if path and os.path.isfile(path):
        with open(path) as fh:
            saved = json.load(fh)
        for name in theta:
            if name in saved:
                theta[name] = float(saved[name])
    return theta


def save_theta(path, theta, iteration):
    if not path:
        return
    with open(path, "w") as fh:
        json.dump({**theta, "_iteration": iteration}, fh, indent=2)


def clip(value, lo, hi):
    return max(lo, min(hi, value))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("engine")
    ap.add_argument("--iterations", type=int, default=2000)
    ap.add_argument("--games-per-iter", type=int, default=6)
    ap.add_argument("--nodes", type=int, default=6000)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--out", default="spsa_theta.json")
    ap.add_argument("--report-every", type=int, default=20)
    ap.add_argument("--alpha", type=float, default=0.602)
    ap.add_argument("--gamma", type=float, default=0.101)
    ap.add_argument("--c-frac", type=float, default=0.05)
    ap.add_argument("--a-ratio", type=float, default=0.25)
    args = ap.parse_args()

    if not os.path.isfile(args.engine) or not os.access(args.engine, os.X_OK):
        sys.exit(f"error: not an executable engine binary: {args.engine}")

    probe = Engine(args.engine)
    params = discover_params(probe)
    probe.close()
    if not params:
        sys.exit("error: engine exposes no tunable spin options")

    rng = random.Random(args.seed)
    theta = load_theta(args.out, params)
    start_iter = 0
    if os.path.isfile(args.out):
        with open(args.out) as fh:
            start_iter = int(json.load(fh).get("_iteration", 0))

    bounds = {p["name"]: (p["min"], p["max"]) for p in params}
    c0 = {name: max(0.5, (hi - lo) * args.c_frac) for name, (lo, hi) in bounds.items()}
    a0 = {name: c0[name] * args.a_ratio for name in c0}
    big_a = args.iterations / 10.0

    openings = load_openings(2000, args.seed)

    print(f"tuning {len(params)} parameters: {', '.join(theta)}")
    print(f"starting from iteration {start_iter}, theta={theta}")

    engine_a = Engine(args.engine)
    engine_b = Engine(args.engine)

    for k in range(start_iter, args.iterations):
        c_k = {name: c0[name] / (k + 1) ** args.gamma for name in theta}
        a_k = {name: a0[name] / (k + 1 + big_a) ** args.alpha for name in theta}
        delta = {name: (1 if rng.random() < 0.5 else -1) for name in theta}

        plus = {
            name: clip(theta[name] + c_k[name] * delta[name], *bounds[name])
            for name in theta
        }
        minus = {
            name: clip(theta[name] - c_k[name] * delta[name], *bounds[name])
            for name in theta
        }

        engine_a.new_game()
        engine_b.new_game()
        for name, value in plus.items():
            engine_a.set_option(name, round(value))
        for name, value in minus.items():
            engine_b.set_option(name, round(value))

        wins = losses = draws = 0
        pairs = (args.games_per_iter + 1) // 2
        for i in range(pairs):
            fen = openings[(k * pairs + i) % len(openings)]
            for a_is_white in (True, False):
                white, black = (engine_a, engine_b) if a_is_white else (engine_b, engine_a)
                winner = play_game(white, black, fen, args.nodes, None)
                if winner is None:
                    draws += 1
                elif (winner == chess.WHITE) == a_is_white:
                    wins += 1
                else:
                    losses += 1

        n = wins + losses + draws
        score = (wins + 0.5 * draws) / n if n else 0.5
        y = score - 0.5

        for name in theta:
            grad = (y / c_k[name]) * delta[name]
            theta[name] = clip(theta[name] + a_k[name] * grad, *bounds[name])

        if (k + 1) % args.report_every == 0 or k + 1 == args.iterations:
            print(f"iter {k + 1}/{args.iterations}  score {score:.3f} (+{wins} ={draws} -{losses})")
            print(f"  theta = {{{', '.join(f'{n}: {v:.1f}' for n, v in theta.items())}}}")
            save_theta(args.out, theta, k + 1)

    save_theta(args.out, theta, args.iterations)
    engine_a.close()
    engine_b.close()


if __name__ == "__main__":
    main()
