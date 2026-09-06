# What is remembered between launches

## Two files, two jobs

The application keeps what it owns under the platform's state directory, found with the
`directories` crate from the three words in `src/identity.rs` rather than from paths written by
hand:

| Platform | Directory                                            |
| -------- | ---------------------------------------------------- |
| macOS    | `~/Library/Application Support/app.canmi.rdm/` for both |
| any, dev | the same with `.dev` after the last word; see below    |
| Linux    | state in `$XDG_STATE_HOME/rdm/`, config in `$XDG_CONFIG_HOME/rdm/` |
| Windows  | state in `%LOCALAPPDATA%\canmi\rdm\`, config in `%APPDATA%\canmi\rdm\` |

Linux distinguishes state from data and the others do not; the code asks for the state directory
and falls back to the local data directory, so on every platform the files land where that
platform's own applications put the same kind of thing.

**A development build keeps its own.** The last of the three words is spelled `rdm.dev` in a
build with `debug_assertions` on -- which the dev profile leaves on, and which is the same thing
the control socket answers to -- so `mise run dev` writes to `app.canmi.rdm.dev` and the
installed application to `app.canmi.rdm`, and neither can overwrite the other's state, config or
database. The identifier the system is given follows it, `app.canmi.rdm.dev`, and Settings shows
that one, so a window that is a development build says which it is where somebody would look.
Two things do not move: `APPLICATION` itself, because the CDN path and the release's files are
spelled from it, and `BUNDLE_ID`, because `.mise/tasks/bundle` reads that line out of the source
with a regular expression and `Info.plist` must carry the published identifier. The downloads
folder is shared, being the user's own and not this application's to fork. The cost of the
isolation is that a development build starts empty the first time it runs after this, with
whatever was there before waiting in the installed application's directory. None of it is meant to be edited by hand:
this is the window's memory, not the user's configuration, and a settings file a user is meant to
open would be a third file with its own rules.

Three files, because three kinds of writing:

- **`state.json`** is small and rewritten whole: the window's frame and whether it was maximised,
  the column widths, the view, whether the header's funnel is lit (see [ui.md](ui.md)), and the
  build that last ran, which the next reads to know what it
  came after (see [release.md](release.md) on the old names). Written a third of a second after the last change, to a sibling
  and then renamed over the old file, so a crash mid-write leaves the previous state rather than
  half of the new one. It is written on change and not at quit, because a forced quit gives no
  moment to write in, and losing a drag's worth of change is the most that can be lost.
- **`internal.sqlite`** holds the downloads, one row each, at schema version 5: version 1's
  columns, `connections` from version 2, and from version 3 `directory`, `mirrors` as a JSON
  list, `checksum`, `range` as written and `speed_limit`, everything Add Task can ask for, NULL
  where it did not; an older file gains the columns on open. Version 4 adds a second table,
  `archives`: for every archive among the rows that can be listed without unpacking -- zip and
  what is a zip under another name, 7z, tar, and a gzip tar under 64 MB, since gzip has no
  directory and must be inflated to its end -- its entries as JSON, keyed by path with the
  file's modification time and size, or the reason it could not be read, so a file is read once
  and again only when it changed. Read in the background after launch, after a download
  finishes, and after the folder is read, one file at a time; a file gone from disk takes its
  row with it. Version 5 adds a third, `notices`: what the system was last told about an update
  and at which stage -- the version and the build named -- one row a stage, replaced rather than
  added to. It is a table and not a line in `state.json` because the point of it is to outlive
  the run that wrote it, and because the check runs every five minutes while the state file is
  written a moment after each change; a notice is a fact about the past, which is what the
  database is for. See [release.md](release.md). The categories judge an archive by what it
  holds as well as by its name, see
  [ui.md](ui.md). See [engine.md](engine.md) and the store.
- **`config.json`**, in the platform's *configuration* directory rather than its state directory,
  is the user's: the categories, and the switches the settings sheet offers, each with a default
  so a file from before a switch reads as if it had been left alone. It is seeded with the built-in
  categories the first time the application starts and finds no file, so a user who wants to
  change the defaults finds them written down rather than baked in, and it is otherwise only
  written when the application itself changes it -- adding a category, switching a preset,
  amending its list, recoloring, reordering. A preset is written as its name and the user's
  changes to its extension list, never as the list itself, so the built-in list can grow under
  it. The file also records every preset it has been offered, which is what lets a preset added
  to the application after the file was written reach it on the next load while one the user
  removed stays removed; a file from before the record is read as having been offered what it
  holds. Its color is written only when it is not the preset's own; a custom rule always carries its
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

`state.json` carries `"version": 2`, and the rule for moving it is the rule of a database
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

Version 2 is the first such change, and it is the shape of one: the Compact view was dropped, and
an enum with a variant taken out of it can no longer read the name of that variant. One field it
cannot read fails the whole object, so a file naming Compact would have cost its reader the
window's frame and the column widths too. The arm rewrites the name to Detailed and moves on.

## A window comes back to the display it was left on

**Which display is remembered, not only where the window was.** A desktop is one plane and the
displays move about in it: unplug a second monitor and plug it in on the other side, or change
which one the system calls first, and the coordinates that meant "the top left of the right-hand
screen" now mean somewhere else, or nowhere. So the file keeps the display beside the frame --
the name the system keeps for it across a restart and a replug, and where that display sat at the
time -- and the frame is read as an offset into it. Both are written on every move and resize,
like everything else here, because there is no hook for a forced quit.

Coming back, the display with that name is found among the ones there are now and the window is
put at the same offset into it, wherever it has moved to. A display that came back smaller keeps
the window whole rather than showing a corner of it: a side that still fits keeps its length and
is pulled in until it is on the screen, and only a side that cannot fit is cut down to what there
is.

**A display that is not there falls back twice.** First to the older rule -- the coordinates as
they were, used when any of the window would land on any screen present -- and then to centring.
A window saved on an external monitor that is unplugged would otherwise come back off-screen with
no edge to grab, and centring is the one answer that is always visible. A system that has no name
to keep for a screen records none, and is read the older way throughout.

The display is a field added beside the others, so it does not move the version: a file written
before it says nothing, which reads as the older way, and that is the correct answer for it.

**Two things had to be worked out rather than asked for.** GPUI reads a window's display once,
when the window is made, and macOS answers nothing for a window that is not on screen yet -- so
the answer is nothing at launch and stays nothing however far the window is dragged afterwards.
And GPUI's macOS display reads `CGDisplayBounds`, whose rectangle is in the same desktop
coordinates a window's frame is in, and then returns it with the origin thrown away: every
display comes back at `(0, 0)`, which says how big each screen is and nothing about where it is.
On a desk with three of them that is three rectangles at the same place.

So the origin is asked of the system again, in `src/screens.rs`, and only the size is taken from
GPUI; and the display a window is on is worked out from the frames -- the one the window is
mostly on, by area. That also answers a window straddling two screens, which no single call
answers well.

## The identifier

The bundle identifier is `app.canmi.rdm`: the domain kept for software, reversed, then the
application, all lowercase -- the shape macOS expects and the one every other application on the
machine uses. It is written in one place, `src/identity.rs`, and the bundle task reads it from there
into `Info.plist` (see [packaging.md](packaging.md)); during development the application runs as
a bare binary and the identifier is not in play. The same three words name the state directory, so
the identifier and the directory agree by construction rather than by remembering to keep them so.

## Starting with the machine

Off to begin with: an application that put itself in the login items without being asked would be
one of those applications. Each system keeps them somewhere different and none of them is a
library call worth a dependency -- macOS reads a launch agent out of `~/Library/LaunchAgents`,
Windows a value out of the current user's `Run` key written with the `reg` that ships with it,
Linux a desktop entry out of `~/.config/autostart`. All three are the user's own, need no
privileges, and are undone by taking them away. The entry is named after this build's identifier,
so a development build and an installed one keep separate entries and neither turns the other on.
What is written points at the binary as it is running: a build moved afterwards starts nothing,
which is better than starting whatever is now at the old path. The switch shows what the system
says after the attempt, so a write that failed reads as off rather than as on.
