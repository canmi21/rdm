#!/usr/bin/env python3
"""Names the build by the day it is made: the UTC date as a version, YYYY.M.D without leading
zeros since semver forbids them, written into Cargo.toml in place of the 0.0.0 the repository
keeps. Run by the release workflow before it compiles, never by hand. Prints the version, and
appends it to GITHUB_ENV as RDM_VERSION when there is one. See spec/release.md.
"""

import datetime
import os
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]


def main() -> int:
	now = datetime.datetime.now(datetime.timezone.utc)
	version = f"{now.year}.{now.month}.{now.day}"
	cargo = ROOT / "Cargo.toml"
	text = cargo.read_text()
	replaced, n = re.subn(r'^version = "0\.0\.0"$', f'version = "{version}"', text, count=1, flags=re.MULTILINE)
	if n != 1:
		raise SystemExit("error: Cargo.toml does not carry version 0.0.0; the repository keeps that and only this writes it")
	cargo.write_text(replaced)
	if env := os.environ.get("GITHUB_ENV"):
		with open(env, "a") as f:
			f.write(f"RDM_VERSION={version}\n")
	print(version)
	return 0


if __name__ == "__main__":
	sys.exit(main())
