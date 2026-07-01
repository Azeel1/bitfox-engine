#!/usr/bin/env python3
import argparse
import datetime as dt
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TRAINER = ROOT / "trainer"

RESULT_RE = re.compile(
    r"(?P<games>\d+)\s+games\s+"
    r"\+(?P<wins>\d+)\s+="
    r"(?P<draws>\d+)\s+-"
    r"(?P<losses>\d+)\s+"
    r"(?P<elo>[+-]?\d+(?:\.\d+)?)\s+\+/-\s*"
    r"(?P<err>\d+(?:\.\d+)?)\s+Elo\s+"
    r"LOS\s+(?P<los>\d+(?:\.\d+)?)%"
)


def run(cmd, cwd=ROOT, env=None, log_path=None):
    print("+ " + " ".join(str(part) for part in cmd), flush=True)
    if log_path is None:
        subprocess.run(cmd, cwd=cwd, env=env, check=True)
        return ""

    log_path.parent.mkdir(parents=True, exist_ok=True)
    text = []
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
            text.append(line)
        code = proc.wait()
    if code != 0:
        raise subprocess.CalledProcessError(code, cmd)
    return "".join(text)


def copy_source(dst):
    if dst.exists():
        shutil.rmtree(dst)

    def ignore(path, names):
        blocked = {
            ".git",
            ".DS_Store",
            "cache",
            "config.json",
            "image.png",
            "target",
            "checkpoints",
            "runs",
            "tmp",
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


def parse_checkpoint_set(value):
    if not value or value == "all":
        return None
    selected = set()
    for part in value.split(","):
        part = part.strip()
        if not part:
            continue
        if "-" in part:
            start, end = part.split("-", 1)
            selected.update(range(int(start), int(end) + 1))
        else:
            selected.add(int(part))
    return selected


def checkpoint_number(path):
    match = re.search(r"-(\d+)$", path.name)
    if match:
        return int(match.group(1))
    return 0


def find_checkpoints(run_dir, selected):
    checkpoints = []
    for path in sorted((run_dir / "checkpoints").iterdir(), key=checkpoint_number):
        if not path.is_dir():
            continue
        number = checkpoint_number(path)
        if selected is not None and number not in selected:
            continue
        network = path / "quantised.bin"
        if network.exists():
            checkpoints.append((number, path, network))
    if not checkpoints:
        raise FileNotFoundError(f"no matching checkpoints found under {run_dir / 'checkpoints'}")
    return checkpoints


def build_candidate(source, network, candidate):
    target_net = source / "engine" / "networks" / "bitfox.nnue"
    target_net.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(network, target_net)
    os.utime(target_net, None)
    run(["cargo", "build", "--release", "--manifest-path", str(source / "engine" / "Cargo.toml")])
    exe = source / "engine" / "target" / "release" / "bitfox"
    if sys.platform == "win32":
        exe = exe.with_suffix(".exe")
    if not exe.exists():
        raise FileNotFoundError(f"candidate binary not found: {exe}")
    candidate.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(exe, candidate)
    candidate.chmod(candidate.stat().st_mode | 0o111)


def parse_result(text, verdict):
    matches = list(RESULT_RE.finditer(text.replace("\r", "\n")))
    if not matches:
        raise ValueError("could not parse selfplay result")
    data = matches[-1].groupdict()
    return {
        "games": int(data["games"]),
        "wins": int(data["wins"]),
        "draws": int(data["draws"]),
        "losses": int(data["losses"]),
        "elo": float(data["elo"]),
        "error": float(data["err"]),
        "los": float(data["los"]),
        "verdict": verdict,
    }


def parse_verdict(text):
    for line in reversed(text.splitlines()):
        if line.startswith("verdict:"):
            return line.split(":", 1)[1].strip()
    return ""


def write_summary(path, rows):
    headers = [
        "checkpoint",
        "games",
        "wins",
        "draws",
        "losses",
        "elo",
        "error",
        "los",
        "verdict",
        "network",
        "candidate",
        "log",
    ]
    with path.open("w", encoding="utf-8") as fh:
        fh.write("\t".join(headers) + "\n")
        for row in rows:
            fh.write("\t".join(str(row.get(header, "")) for header in headers) + "\n")


def parse_args():
    ap = argparse.ArgumentParser(description="Match saved checkpoints against the current engine.")
    ap.add_argument("run_dir", help="training run directory, for example trainer/runs/gen6-m5-59m-wdl035")
    ap.add_argument("--checkpoints", default="1-8", help="checkpoint numbers, for example 1-8 or 4,5,6")
    ap.add_argument("--baseline-bin", default=str(ROOT / "engine" / "target" / "release" / "bitfox"))
    ap.add_argument("--games", type=int, default=80)
    ap.add_argument("--nodes", type=int, default=10_000)
    ap.add_argument("--seed", type=int, default=20260630)
    ap.add_argument("--openings-limit", type=int, default=2000)
    ap.add_argument("--builtin-openings", action="store_true")
    ap.add_argument("--out-dir")
    return ap.parse_args()


def main():
    args = parse_args()
    run_dir = Path(args.run_dir).resolve()
    selected = parse_checkpoint_set(args.checkpoints)
    checkpoints = find_checkpoints(run_dir, selected)

    baseline = Path(args.baseline_bin).resolve()
    if not baseline.exists():
        run(["cargo", "build", "--release", "--manifest-path", str(ROOT / "engine" / "Cargo.toml")])
    if not baseline.exists():
        raise FileNotFoundError(f"baseline binary not found: {baseline}")

    out_dir = Path(args.out_dir).resolve() if args.out_dir else run_dir / "checkpoint-matches" / "latest"
    source = out_dir / "build" / "source"
    logs = out_dir / "logs"
    candidates = out_dir / "candidates"
    if out_dir.exists():
        shutil.rmtree(out_dir)
    logs.mkdir(parents=True, exist_ok=True)
    candidates.mkdir(parents=True, exist_ok=True)
    copy_source(source)

    rows = []
    for number, checkpoint, network in checkpoints:
        label = f"cp{number:02d}"
        candidate = candidates / f"{label}-bitfox"
        log_path = logs / f"{label}-selfplay.txt"
        print(f"\n== checkpoint {number}: {checkpoint.name} ==", flush=True)
        build_candidate(source, network, candidate)

        cmd = [
            "python3",
            str(ROOT / "tools" / "selfplay.py"),
            str(baseline),
            str(candidate),
            "--games",
            str(args.games),
            "--nodes",
            str(args.nodes),
            "--seed",
            str(args.seed),
            "--openings-limit",
            str(args.openings_limit),
        ]
        if args.builtin_openings:
            cmd.append("--builtin-openings")
        text = run(cmd, log_path=log_path)
        verdict = parse_verdict(text)
        result = parse_result(text, verdict)
        result.update(
            {
                "checkpoint": number,
                "network": network,
                "candidate": candidate,
                "log": log_path,
            }
        )
        rows.append(result)

    rows.sort(key=lambda row: (row["elo"], row["los"]), reverse=True)
    write_summary(out_dir / "summary.tsv", rows)
    best = rows[0]
    print("\nBest checkpoint:")
    print(
        f"checkpoint {best['checkpoint']}  "
        f"+{best['wins']} ={best['draws']} -{best['losses']}  "
        f"{best['elo']:+.1f} +/- {best['error']:.1f} Elo  "
        f"LOS {best['los']:.1f}%"
    )
    print(f"summary: {out_dir / 'summary.tsv'}")
    print(f"created_utc: {dt.datetime.now(dt.timezone.utc).isoformat()}")


if __name__ == "__main__":
    main()
