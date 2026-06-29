#!/usr/bin/env python3
import argparse
import datetime as dt
import hashlib
import os
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TRAINER = ROOT / "trainer"


def run(cmd, cwd=ROOT, env=None, log_path=None):
    text = " ".join(str(part) for part in cmd)
    print(f"+ {text}", flush=True)
    if log_path is None:
        subprocess.run(cmd, cwd=cwd, env=env, check=True)
        return

    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            cmd,
            cwd=cwd,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
        )
        assert proc.stdout is not None
        for line in proc.stdout:
            print(line, end="", flush=True)
            log.write(line)
        code = proc.wait()
    if code != 0:
        raise subprocess.CalledProcessError(code, cmd)


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def latest_quantised(checkpoints):
    nets = sorted(
        checkpoints.rglob("quantised.bin"),
        key=lambda path: path.stat().st_mtime,
        reverse=True,
    )
    if not nets:
        raise FileNotFoundError(f"no quantised.bin found under {checkpoints}")
    return nets[0]


def copy_source(dst):
    if dst.exists():
        shutil.rmtree(dst)

    def ignore(path, names):
        blocked = {
            ".git",
            ".DS_Store",
            "target",
            "checkpoints",
            "runs",
            "__pycache__",
        }
        rel = Path(path).resolve().relative_to(ROOT)
        ignored = set()
        for name in names:
            full = Path(path) / name
            if name in blocked:
                ignored.add(name)
            elif rel == Path("trainer") and name == "data":
                ignored.add(name)
            elif full.suffix in {".data", ".log", ".tmp", ".prof", ".trace"}:
                ignored.add(name)
        return ignored

    shutil.copytree(ROOT, dst, ignore=ignore)


def build_candidate(network, build_dir):
    source = build_dir / "source"
    copy_source(source)
    target_net = source / "engine" / "networks" / "bitfox.nnue"
    target_net.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(network, target_net)
    run(["cargo", "build", "--release", "--manifest-path", str(source / "engine" / "Cargo.toml")])
    exe = source / "engine" / "target" / "release" / "bitfox"
    if sys.platform == "win32":
        exe = exe.with_suffix(".exe")
    if not exe.exists():
        raise FileNotFoundError(f"candidate binary not found: {exe}")
    return exe


def write_summary(path, items):
    lines = [f"{key}: {value}" for key, value in items if value is not None]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def parse_args():
    ap = argparse.ArgumentParser(description="Run a Bitfox network experiment.")
    ap.add_argument("name", help="experiment name, for example gen1")
    ap.add_argument("--positions", type=int, default=1_000_000)
    ap.add_argument("--nodes", type=int, default=10_000)
    ap.add_argument("--openings")
    ap.add_argument("--run-root", default=str(TRAINER / "runs"))
    ap.add_argument("--data-text")
    ap.add_argument("--data-bin")
    ap.add_argument("--network")
    ap.add_argument("--skip-datagen", action="store_true")
    ap.add_argument("--skip-convert", action="store_true")
    ap.add_argument("--skip-train", action="store_true")
    ap.add_argument("--skip-match", action="store_true")
    ap.add_argument("--superbatches", type=int, default=8)
    ap.add_argument("--batches-per-superbatch", type=int, default=1000)
    ap.add_argument("--threads", type=int, default=6)
    ap.add_argument("--save-rate", type=int, default=2)
    ap.add_argument("--baseline-bin")
    ap.add_argument("--candidate-bin")
    ap.add_argument("--games", type=int, default=400)
    ap.add_argument("--match-nodes", type=int, default=20_000)
    ap.add_argument("--builtin-openings", action="store_true")
    return ap.parse_args()


def main():
    args = parse_args()
    run_dir = Path(args.run_root).resolve() / args.name
    data_dir = run_dir / "data"
    checkpoints = run_dir / "checkpoints"
    networks = run_dir / "networks"
    logs = run_dir / "logs"
    build_dir = run_dir / "build"
    for path in (data_dir, checkpoints, networks, logs, build_dir):
        path.mkdir(parents=True, exist_ok=True)

    data_text = Path(args.data_text).resolve() if args.data_text else data_dir / f"{args.name}.txt"
    data_bin = Path(args.data_bin).resolve() if args.data_bin else data_dir / f"{args.name}.data"

    if not args.skip_datagen:
        cmd = [
            "cargo",
            "run",
            "--release",
            "--manifest-path",
            str(ROOT / "engine" / "Cargo.toml"),
            "--",
            "datagen",
            str(args.positions),
            str(args.nodes),
            str(data_text),
        ]
        if args.openings:
            cmd.append(str(Path(args.openings).resolve()))
        run(cmd)

    if not args.skip_convert:
        if not data_text.exists():
            raise FileNotFoundError(f"training text not found: {data_text}")
        run(
            [
                "cargo",
                "run",
                "--release",
                "--manifest-path",
                str(TRAINER / "convert" / "Cargo.toml"),
                "--",
                str(data_text),
                str(data_bin),
            ]
        )

    if not args.skip_train:
        if not data_bin.exists():
            raise FileNotFoundError(f"training data not found: {data_bin}")
        env = os.environ.copy()
        env.update(
            {
                "BITFOX_TRAIN_ID": args.name,
                "BITFOX_TRAIN_DATA": str(data_bin),
                "BITFOX_TRAIN_OUTPUT": str(checkpoints),
                "BITFOX_TRAIN_THREADS": str(args.threads),
                "BITFOX_TRAIN_SUPERBATCHES": str(args.superbatches),
                "BITFOX_TRAIN_BATCHES_PER_SUPERBATCH": str(args.batches_per_superbatch),
                "BITFOX_TRAIN_SAVE_RATE": str(args.save_rate),
            }
        )
        run(["cargo", "run", "--release", "--manifest-path", str(TRAINER / "Cargo.toml"), "--bin", "train"], env=env)

    network = Path(args.network).resolve() if args.network else None
    if network is None and not args.skip_train:
        network = latest_quantised(checkpoints)

    exported = None
    if network is not None:
        if not network.exists():
            raise FileNotFoundError(f"network not found: {network}")
        exported = networks / f"{args.name}.nnue"
        shutil.copy2(network, exported)
        (exported.with_suffix(exported.suffix + ".sha256")).write_text(
            f"{sha256(exported)}  {exported.name}\n",
            encoding="utf-8",
        )

    if not args.skip_match:
        baseline = Path(args.baseline_bin).resolve() if args.baseline_bin else ROOT / "engine" / "target" / "release" / "bitfox"
        if not args.baseline_bin:
            run(["cargo", "build", "--release", "--manifest-path", str(ROOT / "engine" / "Cargo.toml")])
        if not baseline.exists():
            raise FileNotFoundError(f"baseline binary not found: {baseline}")

        if args.candidate_bin:
            candidate = Path(args.candidate_bin).resolve()
        elif exported is not None:
            candidate = build_candidate(exported, build_dir)
        else:
            raise FileNotFoundError("candidate binary or network is required for match")
        if not candidate.exists():
            raise FileNotFoundError(f"candidate binary not found: {candidate}")

        cmd = [
            "python3",
            str(ROOT / "tools" / "selfplay.py"),
            str(baseline),
            str(candidate),
            "--games",
            str(args.games),
            "--nodes",
            str(args.match_nodes),
        ]
        if args.builtin_openings:
            cmd.append("--builtin-openings")
        run(cmd, log_path=logs / "selfplay.txt")

    write_summary(
        run_dir / "summary.txt",
        [
            ("name", args.name),
            ("created_utc", dt.datetime.now(dt.timezone.utc).isoformat()),
            ("positions", args.positions if not args.skip_datagen else None),
            ("datagen_nodes", args.nodes if not args.skip_datagen else None),
            ("data_text", data_text),
            ("data_bin", data_bin),
            ("network", exported),
            ("network_sha256", sha256(exported) if exported is not None else None),
            ("superbatches", args.superbatches if not args.skip_train else None),
            ("batches_per_superbatch", args.batches_per_superbatch if not args.skip_train else None),
            ("match_games", args.games if not args.skip_match else None),
            ("match_nodes", args.match_nodes if not args.skip_match else None),
        ],
    )
    print(f"experiment written to {run_dir}")


if __name__ == "__main__":
    main()
