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
release at the commit. The release itself was created once by hand, marked prerelease and given
its one paragraph of notes -- left empty, GitHub shows the commit message there instead -- and
is only ever updated: a nightly that is not a prerelease would be what `/releases/latest`
answers with, and that address is the daily channel's.

## What a build contains

| Target | File | Shape |
| --- | --- | --- |
| `macos-arm64` | `rdm-nightly-macos-arm64.dmg` | the installer window: `Downloads.app` beside an Applications shortcut, see spec/packaging.md |
| `windows-x64` | `rdm-nightly-windows-x64.zip` | `Refined Download Manager.exe` alone, the icon inside it |
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

## The application notices a newer build, and says so

`src/update.rs` reads the channel's `latest.json` and compares its `build` with the number this
binary was made as, `identity::BUILD`; nothing else in the file decides anything. The window
asks at launch and every five minutes after, `update::EVERY`, and asks only for that one small
file, so a build is noticed even while the files themselves cannot be fetched. A check is a
future on the engine's tokio runtime, `Engine::run`, whose answer the window polls the way it
polls events. `Check now` in Settings asks at once; a check asked for while one is under way
joins it.

**Every file has two addresses, and where the reader is picks the first to try.** GitHub's
own, `github.com/canmi21/rdm/releases/download/<tag>/<file>`, and the author's CDN,
`cdn.ffoni.com/github/release/rdm/<tag>/<file>`. Before the first check the window asks
Cloudflare's trace on two of the author's hosts, `canmi.net` then `cdn.ffoni.com`, each the
other's backup, and reads `loc`: `CN` puts the CDN first, anywhere else -- and no answer --
puts GitHub first. The other address is the fallback either way, since GitHub has its outages
and a CDN its gaps. The region is asked once per run. The manifest is not read through
jsDelivr, which was considered: its `gh` endpoint serves a repository's tree at a tag, not the
files uploaded to a release, and `latest.json` is only the latter.

**A newer build is a card in the corner, and a notification when the window is not in
front.** The card sits over the list above the status bar, names the build and the version,
and offers `Get`, which opens the file's first address in the browser -- the install itself is
not written yet, see below -- and `Later`, which closes the card for that build; the next build
brings it back. When the manifest arrives while the window is not the active one, the system
is told once per build through `notify-rust`: D-Bus on Linux, the notification centre on
macOS, WinRT on Windows. On macOS a notification is delivered only for an installed bundle, so
a binary run from the build tree shows none, quietly. A hand build has no number and is never
behind on its own; only a check asked for shows it what is published, so the development
window is not nagged. The settings row `Check for updates` says how the last check went:
the build found and whether this is it, or why it could not be read. `Update channel` shows
Nightly, the only channel, and gets a picker when there is a second; the choice is kept in
`config.json` as `update_channel`.

## What is decided elsewhere

The daily release -- a dated tag cut from the nightly by a scheduled workflow, with a
changelog -- is not written yet; the application will read its manifest at
`releases/latest/download/latest.json` through the same two addresses.

Replacing the binary is not written yet. Its shape is settled this far: the file is fetched by
the same two addresses in the same order, verified against the manifest's `sha256` before
anything is touched, and then swapped in. On macOS and Linux a running executable can be
replaced with a rename over it. On Windows a running executable cannot be deleted or
overwritten but can be renamed, so the running one is moved aside, the new one is put under
its name, and the old one is removed on the next start; the `self-replace` crate does exactly
this, opening the moved copy with `FILE_FLAG_DELETE_ON_CLOSE`, and is the way to take when the
step is written.

Signing and notarization are skipped: builds are signed ad hoc, and macOS asks the user to
open the first one by hand. Certificates would go into the repository's secrets and a step
into the macOS build when there are some.
