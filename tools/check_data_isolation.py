#!/usr/bin/env python3
"""Run a command with isolated app data and reject writes to the real app-data tree."""

from __future__ import annotations

import os
import stat
import subprocess
import sys
import tempfile
from pathlib import Path


def default_data_root() -> Path:
    if os.name == "nt":
        local_app_data = os.environ.get("LOCALAPPDATA", "").strip()
        if local_app_data:
            return Path(local_app_data) / "StalkerSaveEditor"
        profile = os.environ.get("USERPROFILE", "").strip()
        if profile:
            return Path(profile) / "AppData" / "Local" / "StalkerSaveEditor"
    else:
        xdg_data_home = os.environ.get("XDG_DATA_HOME", "").strip()
        if xdg_data_home:
            return Path(xdg_data_home) / "StalkerSaveEditor"
        home = os.environ.get("HOME", "").strip()
        if home:
            return Path(home) / ".local" / "share" / "StalkerSaveEditor"
    return (Path.cwd() / "StalkerSaveEditor").resolve()


def real_data_roots() -> list[tuple[str, Path]]:
    roots = [("app-data", default_data_root())]
    xdg_data_home = os.environ.get("XDG_DATA_HOME", "").strip()
    home = os.environ.get("HOME", "").strip()
    if xdg_data_home:
        roots.append(("fixpack-data", Path(xdg_data_home) / "stalker-save-editor"))
    elif home:
        roots.append(("fixpack-data", Path(home) / ".local" / "share" / "stalker-save-editor"))

    configured_data = os.environ.get("STALKER_SAVE_EDITOR_DATA", "").strip()
    if configured_data:
        roots.append(("configured-app-data", Path(configured_data)))

    temporary_root = Path(tempfile.gettempdir()).resolve()
    unique: list[tuple[str, Path]] = []
    seen: set[Path] = set()
    for label, root in roots:
        resolved = root.resolve()
        if resolved in seen or (label == "configured-app-data" and is_within(resolved, temporary_root)):
            continue
        seen.add(resolved)
        unique.append((label, resolved))
    return unique


def snapshot_tree(root: Path) -> dict[str, tuple[int, int, int, int, int]]:
    """Capture names and metadata only; never read user file contents."""
    try:
        root_info = root.lstat()
    except FileNotFoundError:
        return {}
    except OSError as error:
        raise RuntimeError(f"could not inspect real app-data tree (errno {error.errno})") from None

    snapshot = {".": metadata(root_info)}
    pending = [root]
    while pending:
        directory = pending.pop()
        try:
            entries = list(os.scandir(directory))
        except OSError as error:
            raise RuntimeError(f"could not inspect real app-data tree (errno {error.errno})") from None
        for entry in entries:
            path = Path(entry.path)
            try:
                info = entry.stat(follow_symlinks=False)
            except OSError as error:
                raise RuntimeError(f"could not inspect real app-data tree (errno {error.errno})") from None
            relative = path.relative_to(root).as_posix()
            snapshot[relative] = metadata(info)
            if stat.S_ISDIR(info.st_mode) and not stat.S_ISLNK(info.st_mode):
                pending.append(path)
    return snapshot


def metadata(info: os.stat_result) -> tuple[int, int, int, int, int]:
    return (info.st_mode, info.st_size, info.st_mtime_ns, info.st_ctime_ns, info.st_ino)


def is_within(path: Path, parent: Path) -> bool:
    try:
        path.resolve().relative_to(parent.resolve())
        return True
    except ValueError:
        return False


def snapshot_roots(roots: list[tuple[str, Path]]) -> dict[tuple[str, str], tuple[int, int, int, int, int]]:
    result = {}
    for label, root in roots:
        for relative, details in snapshot_tree(root).items():
            result[(label, relative)] = details
    return result


def main() -> int:
    try:
        separator = sys.argv.index("--")
    except ValueError:
        print("usage: check_data_isolation.py -- COMMAND [ARG ...]", file=sys.stderr)
        return 2
    command = sys.argv[separator + 1 :]
    if not command:
        print("missing command", file=sys.stderr)
        return 2

    protected_roots = real_data_roots()
    before = snapshot_roots(protected_roots)
    temporary_root = Path(tempfile.gettempdir()).resolve()
    if any(is_within(temporary_root, root) for _, root in protected_roots):
        temporary_root = protected_roots[0][1].parent

    status = 1
    try:
        with tempfile.TemporaryDirectory(prefix="sse-data-isolation-", dir=temporary_root) as temporary:
            isolated_data = Path(temporary) / "app-data"
            isolated_data.mkdir()
            environment = os.environ.copy()
            environment["STALKER_SAVE_EDITOR_DATA"] = str(isolated_data)
            if os.name == "nt":
                environment["LOCALAPPDATA"] = str(Path(temporary) / "local-app-data")
            else:
                environment["XDG_DATA_HOME"] = str(Path(temporary) / "xdg-data")
            environment["PYTHONDONTWRITEBYTECODE"] = "1"
            try:
                status = subprocess.run(command, env=environment, check=False).returncode
            except OSError as error:
                print(f"could not run isolated command (errno {error.errno})", file=sys.stderr)
                status = 1
    except OSError as error:
        print(f"could not create isolated app-data directory (errno {error.errno})", file=sys.stderr)
        status = 1

    after = snapshot_roots(protected_roots)
    changed = sorted(name for name in before.keys() | after.keys() if before.get(name) != after.get(name))
    if changed:
        print("FAIL: real app-data changed while the guarded command ran:", file=sys.stderr)
        for label, name in changed:
            print(f"  {label}/{name}", file=sys.stderr)
        return 1
    if status == 0:
        print("PASS: real app-data tree was unchanged")
    return status


if __name__ == "__main__":
    raise SystemExit(main())
