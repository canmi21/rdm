# What is remembered between launches

## Two files, two jobs

The application keeps what it owns under the platform's state directory, found with the
`directories` crate from the three words in `src/identity.rs` rather than from paths written by
hand:

| Platform | Directory                                            |
| -------- | ---------------------------------------------------- |
| macOS    | `~/Library/Application Support/app.canmi.rdm/` for both |
| Linux    | state in `$XDG_STATE_HOME/rdm/`, config in `$XDG_CONFIG_HOME/rdm/` |
| Windows  | state in `%LOCALAPPDATA%\canmi\rdm\`, config in `%APPDATA%\canmi\rdm\` |

Linux distinguishes state from data and the others do not; the code asks for the state directory
and falls back to the local data directory, so on every platform the files land where that
platform's own applications put the same kind of thing. None of it is meant to be edited by hand:
this is the window's memory, not the user's configuration, and a settings file a user is meant to
open would be a third file with its own rules.

Three files, because three kinds of writing:

- **`state.json`** is small and rewritten whole: the window's frame and whether it was maximised,
  the column widths, the view. Written a third of a second after the last change, to a sibling
  and then renamed over the old file, so a crash mid-write leaves the previous state rather than
  half of the new one. It is written on change and not at quit, because a forced quit gives no
  moment to write in, and losing a drag's worth of change is the most that can be lost.
- **`config.json`**, in the platform's *configuration* directory rather than its state directory,
  is the user's: the categories, and later the settings. It is seeded with the built-in
  categories the first time the application starts and finds no file, so a user who wants to
  change the defaults finds them written down rather than baked in, and it is otherwise only
  written when the application itself changes it -- adding a category, today. A file that is
  there but cannot be read is left exactly as it is and the seed is used for the run, so a hand
  edit that went wrong is not corrected away. It carries the same integer version with the same
  rule as `state.json`, and the two share one reader.
- **`internal.sqlite`** will hold the downloads themselves once they persist: many rows, appended
  and updated one at a time, which is what a database is for and what a JSON file rewritten whole
  is not. The name is decided now, beside the other, so the two are one decision; the file does
  not exist until the store does.

Sort order and the sidebar's filter are not remembered: a launch starts at newest-first and All,
because a filter left on from last time reads as downloads having vanished.

## The version is an integer and means one thing

`state.json` carries `"version": 1`, and the rule for moving it is the rule of a database
migration: **the number changes only when a file written before the change can no longer be read
as it is.** Adding a field, dropping one, renaming nothing -- none of that moves the version,
because a reader fills a missing field with its default and ignores one it does not know, and
`serde` is told to do exactly that. Rearranging the file's shape does.

Each such change adds one arm to `migrate` in `src/state.rs`, from version `n` to `n + 1`, and
bumps `VERSION`; a file is brought forward one arm at a time from whatever version it carries.
The arms are the history of the file's shape and are never removed. A file from a *newer* version
is refused and left alone, not guessed at: the build that wrote it reads it correctly, and
overwriting it here with an older shape would lose what that build knew. A file with no integer
version is refused the same way, since a version that could be missing or fractional is a version
nobody can rely on.

## A frame is restored only onto a display that is there

The saved frame is used when any of it lands on one of the displays present at launch, and the
window is centred otherwise. A window saved on an external monitor that is unplugged would come
back off-screen, with no edge to grab; centring is the one answer that is always visible.

## The identifier

The bundle identifier is `app.canmi.rdm`: the domain kept for software, reversed, then the
application, all lowercase -- the shape macOS expects and the one every other application on the
machine uses. It is written in one place, `src/identity.rs`, and the bundle task reads it from there
into `Info.plist` (see [packaging.md](packaging.md)); during development the application runs as
a bare binary and the identifier is not in play. The same three words name the state directory, so
the identifier and the directory agree by construction rather than by remembering to keep them so.
