from pathlib import Path
import re
import tomllib


ROOT = Path(__file__).resolve().parents[1]
PACKAGES = {"engine": "engine", "trainer": "trainer", "convert": "trainer/convert"}


def project_version(root=ROOT):
    manifest = tomllib.loads((Path(root) / "engine/Cargo.toml").read_text(encoding="utf-8"))
    version = manifest["package"]["version"]
    if not isinstance(version, str) or not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", version):
        raise ValueError("engine version must be major.minor.patch")
    return version


def toolchain(root=ROOT):
    manifest = tomllib.loads((Path(root) / "rust-toolchain.toml").read_text(encoding="utf-8"))
    channel = manifest["toolchain"]["channel"]
    if not isinstance(channel, str) or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", channel):
        raise ValueError("rust-toolchain.toml must pin a numbered Rust toolchain")
    return channel


def cargo_command(action, *, root=ROOT, package="engine", profile=None, extra=()):
    if package not in PACKAGES or action not in {"build", "test", "run", "clippy", "fmt"}:
        raise ValueError("unsupported Cargo package or action")
    cwd = (Path(root) / PACKAGES[package]).resolve()
    command = ["cargo", "+" + toolchain(root), action]
    if action != "fmt":
        command.append("--locked")
        if profile == "release":
            command.append("--release")
        elif profile:
            command.extend(["--profile", profile])
    elif profile:
        raise ValueError("format checks do not use a build profile")
    command.extend(str(value) for value in extra)
    return {"argv": command, "cwd": str(cwd)}

