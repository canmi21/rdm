	# The installer: its background drawn from the SVG with the help line's face fetched from
	# Google Fonts, then dmgbuild lays the window out. dmgbuild lives in a venv of its own under
	# target/, made here, so a runner and a machine take the same path and neither has its
	# python written to. The venv is asked whether it still is one, not whether its files are
	# there: the runner's cache of target/ once handed back a venv whose python no longer knew
	# it was in one, and pip then ran as the system's and was refused. See spec/packaging.md.
	venv=target/installer/venv
	font=target/installer/Kalam-Regular.ttf
	mkdir -p target/installer
	"$venv/bin/python3" -c 'import sys; sys.exit(sys.prefix == sys.base_prefix)' 2>/dev/null || {
		rm -rf "$venv"
		python3 -m venv "$venv"
	}
	"$venv/bin/python3" -c "import dmgbuild" 2>/dev/null || "$venv/bin/python3" -m pip install --quiet dmgbuild
#!/usr/bin/env bash
# Wraps the release binary for one target into dist/: a dmg with an Applications shortcut for
# macOS, the bare exe for Windows, an AppImage and a tarball for Linux. The target names the
# system and the architecture -- macos-arm64, windows-x64, linux-x64, linux-arm64 -- and the
# files are rdm-nightly-<target>.<ext>, with no date in the name so the nightly's links never
# change; the daily release renames them. Expects the release binary, target/release/Downloads, already built. See
# spec/release.md.
set -euo pipefail
target=${1:?usage: package.sh <target>}
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
mkdir -p dist
name="rdm-nightly-$target"
# The version the binary was built as, for the publish job, which may run on another day.
sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1 > dist/version.txt

case "$target" in
macos-arm64)
	swift .mise/tasks/icon
	python3 .mise/tasks/bundle
	# The installer: its background drawn from the SVG with the help line's face fetched from
	# Google Fonts, then dmgbuild lays the window out. dmgbuild lives in a venv of its own under
	# target/, made here, so a runner and a machine take the same path and neither has its
	# python written to. The venv is asked whether it still is one, not whether its files are
	# there: the runner's cache of target/ once handed back a venv whose python no longer knew
	# it was in one, and pip then ran as the system's and was refused. See spec/packaging.md.
	venv=target/installer/venv
	font=target/installer/Kalam-Regular.ttf
	mkdir -p target/installer
	"$venv/bin/python3" -c 'import sys; sys.exit(sys.prefix == sys.base_prefix)' 2>/dev/null || {
		rm -rf "$venv"
		python3 -m venv "$venv"
	}
	"$venv/bin/python3" -c "import dmgbuild" 2>/dev/null || "$venv/bin/python3" -m pip install --quiet dmgbuild
	[ -f "$font" ] || curl -fsSL -o "$font" "https://raw.githubusercontent.com/google/fonts/main/ofl/kalam/Kalam-Regular.ttf"
	swift pkgs/macos/render.swift pkgs/macos/background.svg target/installer/background.png 2 "$font"
	"$venv/bin/python3" -m dmgbuild -s pkgs/macos/installer.py \
		-D "app=$root/target/bundle/Downloads.app" \
		-D "background=$root/target/installer/background.png" \
		"Refined Installer" "dist/$name.dmg"
	;;
windows-x64)
	# The executable under its name in full, in a zip so the download is one file with that
	# name inside it. Python's zipfile, since the runner's shell has no zip.
	rm -rf target/pkg && mkdir -p target/pkg
	cp target/release/Downloads.exe "target/pkg/Refined Download Manager.exe"
	(cd target/pkg && python3 -m zipfile -c "$root/dist/$name.zip" "Refined Download Manager.exe")
	;;
linux-x64 | linux-arm64)
	case "$target" in linux-x64) arch=x86_64 ;; *) arch=aarch64 ;; esac
	# The tarball: the binary, the desktop entry, the icon and a script that installs the three.
	rm -rf target/pkg && mkdir -p target/pkg/rdm
	cp pkgs/linux/rdm.desktop pkgs/linux/install.sh target/pkg/rdm/
	cp target/release/Downloads target/pkg/rdm/rdm
	cp assets/icon-512.png target/pkg/rdm/rdm.png
	tar -czf "dist/$name.tar.gz" -C target/pkg rdm
	# The AppImage: the same files in the shape appimagetool expects, system libraries left to
	# the system. The tool is itself an AppImage and is run extracted, since the runner has no FUSE.
	rm -rf target/appdir && mkdir -p target/appdir/usr/bin
	cp target/release/Downloads target/appdir/usr/bin/rdm
	cp pkgs/linux/rdm.desktop target/appdir/rdm.desktop
	cp assets/icon-512.png target/appdir/rdm.png
	ln -s rdm.png target/appdir/.DirIcon
	ln -s usr/bin/rdm target/appdir/AppRun
	tool="target/appimagetool-$arch.AppImage"
	[ -x "$tool" ] || {
		curl -fsSL -o "$tool" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$arch.AppImage"
		chmod +x "$tool"
	}
	ARCH=$arch "$tool" --appimage-extract-and-run target/appdir "dist/$name.AppImage"
	;;
*)
	echo "unknown target $target" >&2
	exit 2
	;;
esac
ls -la dist
