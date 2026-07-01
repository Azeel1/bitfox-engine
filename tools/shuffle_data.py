import argparse
import os
import sys

import numpy as np

REC = 32


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("output")
    ap.add_argument("inputs", nargs="+")
    ap.add_argument("--buckets", type=int, default=32)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--tmp-dir")
    args = ap.parse_args()

    rng = np.random.default_rng(args.seed)
    tmp_dir = args.tmp_dir or os.path.dirname(os.path.abspath(args.output))
    paths = [os.path.join(tmp_dir, f"shuffle_{i:02d}.tmp") for i in range(args.buckets)]
    buckets = [open(p, "wb") for p in paths]

    total = 0
    for path in args.inputs:
        size = os.path.getsize(path)
        if size % REC:
            sys.exit(f"{path}: size not a multiple of {REC}")
        with open(path, "rb") as f:
            while True:
                chunk = f.read(REC * (1 << 20))
                if not chunk:
                    break
                rows = np.frombuffer(chunk, dtype=np.uint8).reshape(-1, REC)
                assignment = rng.integers(0, args.buckets, len(rows))
                for i in range(args.buckets):
                    part = rows[assignment == i]
                    if len(part):
                        buckets[i].write(part.tobytes())
                total += len(rows)
    for b in buckets:
        b.close()

    with open(args.output, "wb") as out:
        for p in paths:
            rows = np.fromfile(p, dtype=np.uint8).reshape(-1, REC)
            out.write(rows[rng.permutation(len(rows))].tobytes())
            os.remove(p)
    print(f"shuffled {total} records into {args.output}")


if __name__ == "__main__":
    main()
