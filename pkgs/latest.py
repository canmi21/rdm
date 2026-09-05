#!/usr/bin/env python3
"""Writes dist/latest.json: what a build is and what it contains, for the update check. The
version, the build number and the commit name the build; each file in dist/ is listed with
its target and kind read off its name, and its sha256, so the application can pick its own
file and verify it before replacing itself. Usage: latest.py <channel> <version> <build> <sha>.
See spec/release.md.
"""

import hashlib
import json
import pathlib
import re
import sys

DIST = pathlib.Path(__file__).resolve().parents[1] / "dist"
NAME = re.compile(r"^rdm-(?P<channel>[^-]+)-(?P<target>[a-z]+-[a-z0-9]+)\.(?P<kind>tar\.gz|[A-Za-z0-9]+)$")


def main(argv: list[str]) -> int:
	if len(argv) != 4:
		raise SystemExit("usage: latest.py <channel> <version> <build> <sha>")
	channel, version, build, sha = argv
	assets = []
	for path in sorted(DIST.iterdir()):
		match = NAME.match(path.name)
		if not match:
			continue
		digest = hashlib.sha256(path.read_bytes()).hexdigest()
		assets.append({
			"target": match["target"],
			"kind": match["kind"],
			"file": path.name,
			"size": path.stat().st_size,
			"sha256": digest,
		})
	if not assets:
		raise SystemExit("error: nothing in dist/ is named like a build")
	latest = {"channel": channel, "version": version, "build": int(build), "sha": sha, "assets": assets}
	(DIST / "latest.json").write_text(json.dumps(latest, indent=2) + "\n")
	print(json.dumps(latest, indent=2))
	return 0


if __name__ == "__main__":
	sys.exit(main(sys.argv[1:]))
