# Release

## A build is named by the day, ordered by a number, and told apart by a commit

`Cargo.toml` says `0.0.0` and is never bumped by hand. The nightly workflow writes the UTC date
in before it compiles -- `2026.9.5`, without leading zeros, since semver forbids them -- so
every build of one day carries the same version and the version says when it was made. What
orders builds is the workflow's run number, `GITHUB_RUN_NUMBER`, which only grows: it is the
bundle's `CFBundleVersion`, the `build` in `latest.json`, and the one thing the update check
compares. The commit, `GITHUB_SHA`, says exactly what was built. Both reach the binary through
`option_env!` in `src/identity.rs`, so a build made by hand carries neither, and `build.rs`
tells Cargo to rebuild when they change; `mise run ctl state` shows all three.

The run number belongs to the workflow's file name. Renaming `nightly.yml` starts it again
from one, and every published build would then read as newer than the next; the file is not
renamed.

## The nightly is one moving release

`.github/workflows/nightly.yml` runs on every push to `main`. Its concurrency group cancels a
run still building when the next push lands: only the newest commit is worth a nightly, and two
publishing at once would race for the tag. Four builds run as a matrix, each on the system it
is for -- macOS on arm64, Windows on x64, Linux on x64 and arm64 -- with `fail-fast` off, so one
system failing to build leaves the other three to finish and keep their files as artifacts;
publishing waits for all four, since a `latest.json` naming one build with another's file
beside it would be a lie the update check believes. There is no Intel build for macOS, five
years after the last Intel Mac shipped; Windows on ARM runs the x64 build through the
system's translation.

Each build: the date written in, the Lucide icons fetched, `cargo test`, `cargo build
--release`, then `pkgs/package.sh` for its target into `dist/`. Linux builds on Ubuntu's LTS,
whose glibc is the oldest a build runs on; `ubuntu-latest` rolls to the next LTS on its own,
and the ARM runner, which has no rolling label, is moved by hand when GitHub offers the next.
Linux links glibc, not musl, because GPUI opens Vulkan with `dlopen`, which static linking
does not have.

The publish job downloads every artifact and first checks that its own run number is above
the build already published, reading the nightly's `latest.json`; a run cancelled late, or
one that ran long, must not overwrite a newer one's files. Then it writes `latest.json`, moves
the `nightly` tag to the commit with force, uploads every file with `--clobber`, and points the
release at the commit. The release itself was created once by hand, marked prerelease, and is
only ever updated: a nightly that is not a prerelease would be what `/releases/latest`
answers with, and that address is the daily channel's.

## What a build contains

| Target | File | Shape |
| --- | --- | --- |
| `macos-arm64` | `rdm-nightly-macos-arm64.dmg` | `rdm.app` beside an Applications shortcut |
| `windows-x64` | `rdm-nightly-windows-x64.exe` | the executable alone, the icon inside it |
| `linux-x64`, `linux-arm64` | `rdm-nightly-linux-<arch>.AppImage` | the binary, desktop entry and icon; system libraries left to the system |
| `linux-x64`, `linux-arm64` | `rdm-nightly-linux-<arch>.tar.gz` | the binary, desktop entry, icon and `install.sh` |

The names carry no date, so the nightly's links never change; the daily release renames them.
`latest.json` beside them is what the application reads: the channel, version, build and
commit, and one entry per file with its target, kind, size and sha256, so the application can
pick its own file and verify it before replacing itself.

```json
{
  "channel": "nightly",
  "version": "2026.9.5",
  "build": 42,
  "sha": "…",
  "assets": [{ "target": "macos-arm64", "kind": "dmg", "file": "rdm-nightly-macos-arm64.dmg", "size": 0, "sha256": "…" }]
}
```

## What is decided elsewhere

The daily release -- a dated tag cut from the nightly by a scheduled workflow, with a
changelog -- and the application's own update check are not written yet; their shape is
settled only as far as the two addresses the application will read: the nightly's
`releases/download/nightly/latest.json` and the daily's `releases/latest/download/latest.json`,
both plain links and neither the API.

Signing and notarization are skipped: builds are signed ad hoc, and macOS asks the user to
open the first one by hand. Certificates would go into the repository's secrets and a step
into the macOS build when there are some.
