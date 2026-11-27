"""Locate the one local adapter archive shared by linked source worktrees."""
from pathlib import Path
import subprocess


def archive_checkout(checkout: Path) -> Path:
    if not (checkout / ".git").is_file():
        return checkout.resolve()
    common = subprocess.check_output(
        ["git", "-C", str(checkout), "rev-parse", "--path-format=absolute", "--git-common-dir"],
        encoding="utf-8",
    ).strip()
    return Path(common).resolve().parent
