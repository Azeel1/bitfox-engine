#!/usr/bin/env python3
import argparse
import json
import os
from pathlib import Path
import sys

sys.dont_write_bytecode = True

from bitfox_project import ROOT, cargo_command, project_version, toolchain


COMMANDS = ("version", "build", "test", "perft", "bench", "gui", "train", "check")


def specification(command, arguments, root=ROOT):
    root = Path(root).resolve()
    if command == "version":
        if arguments:
            raise ValueError("version takes no arguments")
        return []
    if command in {"train", "check"}:
        scripts = {"train": "trainer/experiment.py", "check": "tools/check_project.py"}
        path = root / scripts[command]
        return [{"argv": [sys.executable, "-B", str(path), *arguments], "cwd": str(root)}]
    if command in {"perft", "bench"}:
        values = arguments or (["6"] if command == "perft" else [])
        return [cargo_command("run", root=root, profile="release", extra=["--bin", "bitfox", "--", command, *values])]
    if command == "gui":
        parser = argparse.ArgumentParser(prog="bitfox gui")
        parser.add_argument("operation", choices=("configure", "build", "test"), nargs="?", default="configure")
        parser.add_argument("--build-dir", type=Path, default=root / "build/gui")
        parser.add_argument("--jobs", type=int, default=1)
        args, extra = parser.parse_known_args(arguments)
        if args.jobs < 1:
            raise ValueError("jobs must be positive")
        build_dir = args.build_dir.expanduser().resolve()
        if extra[:1] == ["--"]:
            extra = extra[1:]
        if args.operation == "configure":
            argv = ["cmake", "-S", str(root / "gui-qt"), "-B", str(build_dir), "-DCMAKE_BUILD_TYPE=Release",
                    "-DBUILD_TESTING=ON", "-DBITFOX_STRICT_WARNINGS=ON", *extra]
        elif args.operation == "build":
            argv = ["cmake", "--build", str(build_dir), "--config", "Release", "--parallel", str(args.jobs), *extra]
        else:
            argv = ["ctest", "--test-dir", str(build_dir), "-C", "Release", "--output-on-failure", *extra]
        return [{"argv": argv, "cwd": str(root)}]
    if command in {"build", "test"}:
        parser = argparse.ArgumentParser(prog="bitfox " + command)
        parser.add_argument("--package", choices=("engine", "trainer", "convert"), default="engine")
        parser.add_argument("--profile", default="release" if command == "build" else "release-test")
        parser.add_argument("--jobs", type=int, default=1)
        args, extra = parser.parse_known_args(arguments)
        if args.jobs < 1:
            raise ValueError("jobs must be positive")
        if extra[:1] == ["--"]:
            extra = extra[1:]
        profile = args.profile
        if command == "test" and args.package != "engine" and profile == "release-test":
            profile = "release"
        values = ["-j", str(args.jobs), *extra]
        return [cargo_command(command, root=root, package=args.package, profile=profile, extra=values)]
    raise ValueError("unsupported command")


def main(argv=None):
    parser = argparse.ArgumentParser(description="Build, verify and run Bitfox tools from one entry point.")
    parser.add_argument("--dry-run", action="store_true", help="print commands without starting any process")
    parser.add_argument("command", choices=COMMANDS)
    parser.add_argument("arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    try:
        commands = specification(args.command, args.arguments)
        if args.dry_run:
            print(json.dumps({"version": project_version(), "toolchain": toolchain(), "commands": commands}, indent=2))
            return 0
        if args.command == "version":
            print(project_version())
            return 0
        spec = commands[0]
        os.chdir(spec["cwd"])
        os.execvp(spec["argv"][0], spec["argv"])
        return 0
    except (OSError, ValueError, KeyError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    sys.exit(main())
