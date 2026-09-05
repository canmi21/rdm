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
  written when the application itself changes it -- adding a category, switching a preset,
  amending its list, recoloring, reordering. A preset is written as its name and the user's
  changes to its extension list, never as the list itself, so the built-in list can grow under
  it, and its color only when it is not the preset's own; a custom rule always carries its
  color as hex. See [ui.md](ui.md). A file that is
  there but cannot be read is left exactly as it is and the seed is used for the run, so a hand
  edit that went wrong is not corrected away. It carries the same integer version with the same
  rule as `state.json`, and the two share one reader.
- **`internal.sqlite`** holds the downloads themselves: one row each -- the address, the page it
  was found on, the name, the size and how much has landed, the status, when it was added,
  where the finished file went, and why it failed if it did. Many rows, appended and updated one
  at a time, which is what a database is for and what a JSON file rewritten whole is not. A row
  is written as it changes and read back at launch; one that was moving or waiting when the
  window closed is queued again and the engine continues it from the plan beside its partial
  file, and one that was paused, failed or done is left as it was. Ids are the store's, one
  above the highest ever used, and the engine takes them as its own, so nothing maps between
  the two and a removed row's id is never handed out again while a partial file might still
  carry it. The schema's version is SQLite's `user_version`, under the same rule as
  `state.json`'s. Speed is not kept; it is a number about now.

A download in flight leaves two files beside where it will land, both the engine's and neither
the window's: `name.downloading`, the bytes, and `name.rdm`, the plan -- which segments there
are and how far each has come. The first suffix says what the file is to anyone who meets it in
a folder; the second names the application. Both exist from the moment the probe answers, before
the first byte, so a crash a second in leaves something to continue. At launch the download
folder is read for plans the database does not know -- left by a run whose rows were lost, or
copied in from another machine -- and each that can be continued comes in as a paused row: a
plan this build reads, that holds together, with its partial file beside it at least as long
as the plan says. Anything else is left exactly where it is and off the list, because a file
the user meant to keep is not this application's to delete and one it cannot read is one it
cannot judge. See [engine.md](engine.md).

**The folder is watched, and a burst of changes is one look.** The operating system reports
what is created, written or removed in the download folder -- FSEvents on macOS, through the
`notify` crate -- and every such event starts, or restarts, a timer of 210 milliseconds; the
folder is read again only when the timer runs down, so a copy of a hundred files is one scan
and not a hundred, while a single file dropped in shows up at once. Reads, changes to metadata
alone and the catch-all events a platform sends for what it cannot name are dropped before
they are counted, since none of them puts a plan in the folder or takes one out. The events
carry the paths they are about, and those alone are looked at -- each that is one of a
download's two files is judged by the rules above, on its own -- rather than the folder being
read again; the read of the whole folder is for launch, when nothing yet says what is there.

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
