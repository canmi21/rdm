#!/usr/bin/env bash
# Wraps the release binary for one target into dist/: a dmg with an Applications shortcut for
# macOS, the bare exe for Windows, an AppImage and a tarball for Linux. The target names the
# system and the architecture -- macos-arm64, windows-x64, linux-x64, linux-arm64 -- and the
# files are rdm-nightly-<target>.<ext>, with no date in the name so the nightly's links never
# change; the daily release renames them. Expects target/release/rdm already built. See
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
	rm -rf target/dmg && mkdir -p target/dmg
	cp -R target/bundle/rdm.app target/dmg/
	ln -s /Applications target/dmg/Applications
	hdiutil create -volname rdm -srcfolder target/dmg -ov -format UDZO "dist/$name.dmg"
	;;
windows-x64)
	cp target/release/rdm.exe "dist/$name.exe"
	;;
linux-x64 | linux-arm64)
	case "$target" in linux-x64) arch=x86_64 ;; *) arch=aarch64 ;; esac
	# The tarball: the binary, the desktop entry, the icon and a script that installs the three.
	rm -rf target/pkg && mkdir -p target/pkg/rdm
	cp target/release/rdm pkgs/linux/rdm.desktop pkgs/linux/install.sh target/pkg/rdm/
	cp assets/icon-512.png target/pkg/rdm/rdm.png
	tar -czf "dist/$name.tar.gz" -C target/pkg rdm
	# The AppImage: the same files in the shape appimagetool expects, system libraries left to
	# the system. The tool is itself an AppImage and is run extracted, since the runner has no FUSE.
	rm -rf target/appdir && mkdir -p target/appdir/usr/bin
	cp target/release/rdm target/appdir/usr/bin/rdm
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
