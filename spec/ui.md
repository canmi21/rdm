# The window

## What it is modelled on

A download manager's window has a settled shape, and rdm keeps to it rather than inventing one:
a sidebar of filters on the left, each with its icon, the list in the middle, actions along the
top and one thin status line along the bottom. Neat Download Manager is the reference for what goes where; Zed is the reference for how tightly. The
parts are separate modules under `src/ui/` named for what they are -- toolbar, sidebar, list,
detail -- so a reader looking for one finds it by name.

## The toolbar owns the titlebar

The system titlebar is transparent and the toolbar runs the full width behind the traffic
lights, one strip instead of two. That makes the traffic lights' vertical position this
application's to get right, and it is derived rather than typed: gpui_macos sizes the button
strip to `button height + 2 * y` and hangs it from the top, so setting `y` to half of
`toolbar height - button diameter` centres the buttons for whatever height the toolbar has. The
diameter, 14 points, was measured from a capture of the window; it is the one number in the
formula that is an observation rather than a choice.

**Where the system draws no frame, the toolbar is the frame.** On Windows the transparent
titlebar leaves the whole strip to the application; on Linux the window asks for client-side
decorations, and a compositor that grants them draws no bar and no buttons at all. In both,
`src/ui/frame.rs` puts minimize, maximize or restore, and close at the strip's right in the
system's arrangement and width, drawn with the application's own glyphs on a ten point grid
at one point of stroke -- Lucide's minus, square and cross read as three weights side by
side -- and each answers its own press through the window: minimize, zoom, close. They are
the application's buttons, not the system's control areas: marking the strip as the system's
caption was tried and the system then took every press on it, buttons included, and read two
of them as a maximize. The one exception is maximize on Windows, which is the system's
control area after all: gpui's zoom there only maximizes and cannot restore, while the
system's own button toggles, and it brings the snap layouts on hover with it. Only the empty middle is the system's on Windows, so it drags and
double-clicks as a caption does; on Linux a press there starts a move through the compositor,
a double press zooms, the right button opens the window's menu, and a press within six points
of the window's edge starts a resize, since with client-side decorations nobody else would.
A compositor that cannot give client-side decorations keeps its own bar, and the strip shows
no controls. Add Task starts at the strip's left edge wherever there are no traffic lights to
clear, which on macOS includes full screen, where the system hides them. A release build on
Windows also says it is a windowed program, or a console opens beside it.

## Four views, Detailed by default

The list draws three ways and a segmented control at the toolbar's right end picks one:

| View       | A row is                                                        | For                       |
| ---------- | --------------------------------------------------------------- | ------------------------- |
| Compact    | one 22px line: type, name, a short bar, size, a status mark     | a long queue              |
| Thumbnails | one 36px line: the system's own icon for the file, name, size   | finding a file by eye     |
| Detailed   | a table row: type, name, size, progress with percent, speed, status | the default; shows it all |
| Grid       | a card with a large type icon, or a picture of the file         | scanning by type          |

They are offered densest first, and the glyph on each button says what a row looks like: bare
lines, lines with a picture on them, a table, cards.

**The picture is the system's own.** Every desktop keeps an icon per kind of file, and it is the
picture somebody already knows the file by, so the thumbnails view asks for it rather than
inventing one -- Word's icon on a `.docx`, Excel's on an `.xlsx`, whatever has claimed the kind.
Where there is none to be had, and on the systems this is not written for yet, the category's own
glyph stands in; that is not a failure, since the glyph is what this application draws when it is
drawing for itself. The pictures are cached by path for the run and asked for again when a
download finishes, the file on disk no longer being what it was.

**A frame asks for two dozen of them and no more.** Asking is a trip to the window server and a
decode, and the list draws every row it has rather than only the ones on screen; a folder of a
thousand files spent a minute in one frame and the window answered nothing until it was over.
With the limit the pictures arrive over the next second or two, a frame at a time, and the window
stays a window meanwhile. A frame that runs out asks for another, which is what keeps them
coming.

Detailed is the default because it is the only one that shows everything at once, which is what
a download manager is open for. It is a table rather than a card list because a card list spends
three lines on what a row says in one, and the density asked for here is an editor's, not a
launcher's. The other two trade completeness for density or for a glance.

**The table's header sorts, and its columns are dragged to width.** The name's title reads left, as its cells
do; every other title reads right, over its numbers, and the chevron's slot sits on the side the
text is not aligned to -- after the name, before a number's title -- so a title's edge is its
column's edge and the mark hangs inward from it. Clicking a title orders by it
ascending, a second click descending, and a third lets go, back to the default: newest first, by
Added, which shows no chevron because nothing has been asked for. The chevron sits in a slot every
title reserves, so ordering by another column does not push its neighbours over -- it did, and
the header jumped. A status cell reads text then mark, so the marks line up down the right edge
under a left-aligned title; at a column's floor the text is wider than the column, so the cell
clips to its own width and the text truncates within it -- the mark is the half that stays,
being what the eye reads down the edge. Titles truncate for the same reason. Without that the
status ran across the date beside it, which a screenshot showed and no headless test could. A handle sits at each column's left edge and
drags that column: the table is anchored at its right and the name column absorbs the rest, so a
column's left edge is the one that can move, and a boundary that follows the pointer is what a
drag means -- the first cut put the handle on the right and read as reversed. The widths live on
the view, and every row spends the same twelve points on the handle's gap so cells stay under
their titles.

**Every column has a floor, and the floors are what the window is measured by.** A column will
not go below a width that leaves its cell merely legible -- "1.2G", a stub of a bar, a truncated
word beside its mark -- and the name column keeps one of the same kind. They are floors, not
widths anyone would choose to stop at, and that is the point: a floor somebody might want is a
floor a drag runs into. Widening takes from the left of the handle and takes only what is above a
floor: the name column first, since it is the one holding the slack, then each fixed column
between it and the handle, nearest first. The boundary stops when everything to its left is on its
floor, and nowhere earlier -- there is no ceiling derived from any one column, so a press changes
nothing until the pointer moves. It did: the stop used to be worked out from the name column's
floor alone, which the window's own default size already breached, so every column's ceiling sat
below the width it already had and the first move of a press snapped it there, twenty-four points
for a one-point nudge. Every floor added together, with the sidebar and the chrome around them, is
the window's minimum width, which the system is told and enforces. Between that width and enough
room for what was asked for, the table is drawn squeezed: the shortfall is shared out in
proportion to what each column has above its floor, so narrowing compresses the table evenly
rather than crushing one column, and at the least width every column is exactly on its floor. The
widths asked for are not overwritten by the squeeze, so widening the window gives back what
narrowing it took, to the point; and a drag that comes to move nothing leaves them alone too, so
taking hold of a handle where the window can give nothing is the same as never having pressed.

A drag works from a snapshot of the whole row taken when the press landed, not from the row the
last move left, so the way back gives back exactly what the way out took, and no error
accumulates. Reading the row as it stands is not the same thing, and the difference is a bug that
was there: the ceiling was worked out against the width the drag started at while the sum it was
subtracted from had already moved, so the ceiling fell as the column rose, the boundary alternated
across the pointer, and each fresh press took half of what was left. A drag is tracked on the
window root, not on the handle, because the pointer leaves the handle the moment it moves; a move
with the button up ends it, since a release outside the window is never seen and would otherwise
leave the next pointer movement resizing on its own. The
corner over the type icons holds a funnel that stays: lit, the lists also hold what else the
download folder holds -- every plain file that is not hidden, not one of a download's two
files meanwhile, and not named by a row -- each as a completed row with the file's size and
time and no address; pressed again, the downloads alone. Such a file is treated as a download
that finished: it is under All Tasks, under Completed, and under whichever category its name
fits, and the sidebar's counts include it. The rows are read when the funnel is lit and again
whenever the folder changes while it is. Whether the funnel is lit is remembered in state.json,
and **lit to begin with**: what the download folder holds is what somebody opening a download
manager came to see, and a first launch that showed only rows this application happens to have
written would look emptier than the folder is. It began the other way, off, on the argument that
a folder of a thousand files is not a first impression; the answer is that the funnel is right
there to press. **The default moves only for somebody who has never chosen.** A save writes the
field whether the funnel was touched or not, so a state.json that names it has been chosen for,
either way, and is left exactly as it is; the field is absent only in a file this application
has never written, which is a first launch. That is why it is an `Option` in `state.rs` rather
than a `bool`: the absence is the only thing that can tell the two apart, and a default that
quietly flipped a user's own "off" back on would be the worst of the three outcomes. Remove on one of them takes it off the
list for the session and leaves the file where it is, since a file that was never downloaded
here is not this application's to delete. The corner held the column widths' reset before,
shown only while the pointer rested on it; that moved to `Reset` under `Column widths` in
Settings' Appearance, so a table dragged out of shape still has one way home rather than five
drags, and the corner could hold something consulted more often. **The status bar's left is the count, and what runs behind the window.** At rest it says how
many downloads there are, or how many are moving; while something runs that the list does not
show -- the update check, a build on its way in or being installed, a read of the folder --
a spinner turns after the count with the first such thing named beside it, and every one of
them in its tooltip, since they stack. A read of the folder that finishes within a moment,
three tenths of a second, earns no spinner: one that only flashed would read as a glitch. The
folder is read off the window's thread for the same reason, so a folder of thousands never
holds a frame. A funnel at the
status bar's corner opens a menu of statuses that cuts within whatever the sidebar selected --
one at a time, with All to clear it -- and the funnel stays lit, naming the status, while one is
chosen. The statuses were a row of chips above the list first, then a row in the status bar, and
ended in a menu: a filter is consulted far less often than the rows are read, so it earns an icon
and a click, not a strip of the window. The sidebar answers "which downloads", the menu answers
"in what state", and neither duplicates the other.

The view, sort and chip are not yet remembered across launches; that waits on there being any
persistence at all.

## Color: Nord, through glass, drained when inactive

The palette is [Nord](https://www.nordtheme.com), in `src/ui/theme.rs`, and every name there
says what a color is for -- `panel`, `border`, `muted`, `selection` -- never what it looks like.
Status is the exception and is color-coded on purpose: aurora green complete, orange paused,
red failed, frost blue for downloading, grey queued, because a column of status words reads
slower than a column of colors. Selection and hover are Nord's polar-night greys; the frost blue
is reserved for progress that is moving.

**Every filter and category owns a color, shown only when asked.** All Tasks is snow white,
the three states take their status colors, and each built-in category starts with one of
Nord's nine accents -- Videos purple, Audio teal, Images yellow, Documents frost, Plain Text
teal, Presentations orange, Spreadsheets green, eBooks green, Code blue, Archives orange,
Programs red, Disk Images navy -- with a custom rule handed the next hue in a fixed cycle so
nobody has to pick. Any of them can be changed to a named hue or one the user writes; the
color is a number on the category, so a written one is the same kind of thing as a named one.

**An icon in the tray, with the system's menu under it.** The menu bar's right on macOS, the
notification area on Windows, the indicator area on Linux: the application's icon, and under
it the system's own menu with two items, show the window and quit. On macOS the icon is the
bare glyph as a template image, which the system tints for a light or dark menu bar, at 22
points; elsewhere the full icon, since those trays draw an icon as it is. A left click on
Windows or Linux shows the window as the first item would; on macOS a click opens the menu,
which is how the menu bar works. Closing the window still quits, as before: the tray is a way
back to the window and a way out, not a place the window hides. A tray that cannot be made --
a Linux desktop with no host for a StatusNotifierItem, say, which is a GNOME without the
extension for it -- is reported and done without. See [packaging.md](packaging.md) for the two
rendered icons and [framework.md](framework.md) for why Linux speaks the bus directly.

**Notifications is a page of one row a moment, and each row is a choice of where.** Four moments
are worth telling somebody about -- a download finishes, a download fails, every download
finishes, a newer build is found -- and each carries its own setting, because what somebody wants
said out loud about a finished download is rarely what they want said about a failed one. The
choices are places and not degrees of loudness: `System notification` reaches somebody who is not
looking at the window, `In the window` is a card in the corner that reaches somebody who is, and
`Nothing` reaches nobody. None of them stands in for another -- a place that cannot show a notice
does not quietly hand it to one that can, since a notice arriving somewhere it was not asked for
is worse than one that does not arrive.

The defaults are what the four moments are worth: a finished download opens the dialog, which is
the one notice with something to do next and the reason the dialog exists; a failed one speaks to
the system, since the point of it is to reach somebody who has looked away and there is nothing
to do but look; every download finishing says nothing, or the last download of a batch would say
it twice; and a newer build shows the card in the corner. That last row is the one asymmetry, and it is deliberate: the
update's card is the only place a build can be installed from, so it stands for every choice but
`Nothing` -- `In the window` is the card alone and `System notification` is the card and the
centre as well. Silencing it silences both. A card in the corner goes on its own after six
seconds or at a press, four at a time at most, oldest first, and the update's card sits under
them in the same column so neither lands on top of the other.

**A window of its own is a dialog in the middle of the screen, and it asks the system for
nothing.** It is `WindowKind::PopUp`, which puts it above the ordinary windows, with no titlebar
and no frame of the system's -- a rounded rectangle, a cross at its top right, and the whole of
it ours -- and it takes no focus, so a notice arriving while somebody is typing does not take the
next keystroke. The middle rather than a corner because this is a thing to answer rather than to
glance at: `Download finish`, the file's name, what it came to and how long it took, and three
things to do about it. `Open` hands the file to whatever the system opens it with; `Show in
Finder` -- `File Explorer` on Windows, whatever the user named under Folder in Settings on
Linux -- shows it where it lives; `Downloads` brings the window forward. Each does its thing and
closes, since a dialog that stayed open after being acted on would be one to close twice. The
cross closes and does nothing else, which is the whole of what a cross promises. It is the one place that reaches somebody whose main
window is closed or buried without going through the system's notification centre, which on macOS
delivers only on behalf of an installed bundle and so says nothing at all from a development
build. Panels stack downward from the corner and are counted rather than reflowed: one going does
not slide the others up, since a notice moving out from under a pointer about to press it is
worse than a gap. A press closes it and brings the application forward, which is what a press on
the system's own notification does. A second dialog while the first is up steps down and right
from it, far enough to see there are two and near enough to read as a stack. Being on a layer
above the ordinary windows, it is invisible
to `mise run shot`, which takes the application's window; `shot --floating` takes this one. See
[workflow.md](workflow.md).

**The window is read in one of three languages**: American English, simplified Chinese and
Japanese. `Language` is the first row under General and takes effect at the next frame, which is
what "immediately" looks like -- nothing is restarted and nothing is rebuilt. `System` is what a
first launch has and what a `config.json` written before this reads as: the machine's own
language decides until somebody picks one, and picking one is picking it for good.

Every string the user reads is a flat key -- `settings.section.network` rather than a tree, three
files that must agree being easier to compare than three trees to walk -- and the files are one a
language under `locales/`, embedded at build time, since a translation that can go missing at run
time is a window that can come up blank. English is the source of truth for the set of keys and
the fallback for anything a translation has not caught up with. A test compares the three key
sets, so a string added in English and forgotten in the others is caught before it is shipped.

**Not everything is translated, on purpose.** A name is a name: `rdm`, `Downloads`, `Finder`,
`Chrome`, `Hickory`, `HTTPS`, `SOCKS5`, `.DS_Store`, `Download finish`. A Chinese or Japanese
sentence with those left in English is what somebody who uses this software writes; one with them
translated is what a machine writes. Debug selectors name the key rather than the text, so a test
means the same thing whatever language the machine running it is set to, and the tests pin
themselves to English for the same reason.

**A settings row is a label, a line saying what it does, and a control**, and the rows are
gathered under headings within their section. The line under the label is the part that was
missing: `Auto update` names itself and says nothing about what happens, and what happened used
to be a second row away. The headings are what make a section of a dozen rows into three short
lists; rows of one group are gathered together whatever order they were written in, since a
group split in two by a row from another gets its heading twice and reads as two lists of the
same name. The search reads the note and the heading as well as the label, because somebody
looking for `proxy` is looking for what a setting does and the label is often the one word that
does not say it. `Updates` has a section of its own, General having been a dozen rows in one run.

The pane scrolls, and the label gives way while the control does not: a note is a sentence and
will take every point it is given, and a control clipped to nothing cannot be pressed. Both were
learned the hard way -- the switches stopped answering, and a row with a long path in it grew to
fifteen hundred points because the note beside it was left a character wide.

**A resolved address fills in the name it will be saved under.** Add Task looks at an address
before anything is fetched -- what it is, how big, whether it can be split -- and the name the
server gave it, or the address's last segment, is written into `Save as` on the face rather than
behind `More`. An empty field beside a resolved address reads as though nothing was resolved, and
a name somebody may want to change is not a thing to hide behind a word; everything else behind
`More` is something most people never touch, and this is not.

**A download remembers where it came from.** The window a row opens says `From`, which is the
address it was fetched from, and `Found on` where it came from a page rather than being typed in.
A row with no address was not downloaded here -- it is a file the folder already held -- and the
window says so rather than showing an empty line.

**An inactive window goes grey, unless asked otherwise.** "Dim the window when it is not in
front", under Appearance and on to start with, is the switch behind the monochrome the palette
takes when the window is not active; off, the window keeps its colors in front or not, for
someone who keeps it beside another window and reads it there. See the palette in
[framework.md](framework.md).

**A category's colour is the outer one; an extension inside it can draw in its own.** The icon
is the category's and never changes -- a document is a document -- but the hue can differ within
it, because a category is often two or three things a person tells apart at a glance. A PDF among
the documents draws red; a machine's disk among the disk images draws frost against the navy an
installer's image keeps; an installer for Windows, for Linux and for a phone each draw apart
inside Programs; a bitstream draws apart from a chip image inside Firmware. What the sidebar shows
is still the category's own colour, since that is the whole of it rather than one of its parts.

An extension with no shade of its own draws in the category's, which is what most of them do and
what leaving a shade empty means. **The list of extensions is where they are set, and it is one
list rather than two.** A chip wears the colour a file of that extension would, so the feature
says what it is by being what it does; `Colors`, at the right of the heading, turns the chips
from switches into doors, the same turn the presets face makes under `Edit`. While one is open
the heading names it, the swatches paint that extension rather than the category, and `Inherit`
gives it back the category's colour; pressing the open chip again, or `Colors` a second time,
puts the swatches back to the category. Nothing is painted while the chips are doors and none is
open: the swatches wait rather than colouring the category by accident. `config.json` writes only what differs from the preset, so a
shade the application adds later reaches a file that never touched one; an extension the user set
back to the category's colour is written as an empty string, which is the only way to tell
"inherit, deliberately" from "never said".

**A download folder's own folders are ignored, flattened, or kept as folders**, under Folder in
Settings. `Ignore them` is what it does to start with, and the reason is a measurement: the
folder this was written in has a checked-out repository in it, and flattening turned eighty-six
rows into fifteen hundred, of which none was downloaded. A directory and its contents are left
out entirely. `Show what is inside` lists every file inside at the top level beside the loose
ones, for a folder somebody really does keep downloads in; nothing moves on disk, this being how
the folder reads and not how it is. `Keep them as folders` gives
the directory a row of its own that opens onto what it holds: one press opens, a second closes,
and a row inside is drawn only while every folder between it and the download folder is open. A
folder row is a door and nothing else, having no window of its own and nothing to act on.

Two limits are the reader's, not the list's: it will not go more than eight folders deep, and it
will not make more than twenty thousand rows. A download folder is not a filesystem, and a folder
nested eight deep in one is not what anybody came for; the depth limit is also what stops a
symlink loop holding the read open. A bundle -- `.app`, `.framework` and the rest -- is a
directory the system draws as one file, and is left as one: reading inside a `.app` would list
its whole contents where the application belongs.

**The folder's junk is kept out of the lists, and a torrent is filed rather than dropped.** A
download folder collects a great deal nobody downloaded: what the operating system leaves behind
(`.DS_Store`, `Thumbs.db`, `desktop.ini`), what an editor writes beside a file it has open
(`~$Report.docx`), the pointers a browser saves instead of a file (`.lnk`, `.url`, `.webloc`),
and what another downloader left half-finished (`.crdownload`, `.part`). None of it is worth a
row, and a list of eighty rows of which nine are `.DS_Store` is a worse list than one of
seventy-one; `Hide the folder's junk`, under Folder in Settings, is on to start with. A torrent
is the one exception, and it is a different kind of exception: worth keeping and worth filing,
but not worth a place among downloads, so it has a row under `Torrents` and nowhere else --
which is where somebody looking for one would look. The name is judged whole and without its
case, since most of these are known by their whole name rather than their extension.

**The presets are the kinds a download folder actually fills with**, and three of them arrived
after the first cut: `Torrents`, `3D Models` and `Firmware`. Firmware took `bin` and `hex` from
`Disk Images`, which had been counting a chip's contents as a filesystem; a disk image is an
optical image, an installer's image or a machine's disk, and a firmware image is neither. Its
list is five families rather than a handful: the toolchain's output (`elf`, `axf`, `out`), the
record formats an assembler emits (`hex`, `ihex`, `srec`, `s19`, `mot`, `sre`), the containers a
flasher takes (`uf2`, `dfu`, `gbl`, `cyacd`, `apj`, `px4`, `swu`, `ota`), the bitstreams an FPGA
or CPLD is programmed with (`rbf`, `sof`, `pof`, `jed`, `svf`, `jam`, `mcs`, `bit`), and the
whole-device images a vendor ships (`rom`, `trx`, `chk`, `ipsw`, `kdz`, `capsule`). Two
extensions were argued over and left out on purpose: `img` is a filesystem far more often than a
chip, and `cap` is a packet capture far more often than a UEFI capsule. A
preset the application learns after a `config.json` was written reaches that file on the next
load, since a category that only exists for somebody starting fresh is not a category the user
has. What they take away stays away: the file records every preset it has been offered, and only
what is missing from that record arrives. See [state.md](state.md).

**An archive is judged by what it holds as well as by its name.** Once the index has read it
-- see [state.md](state.md) for which kinds, and the status bar's spinner while it runs -- an
archive is in every category its own name matches, Archives for one, and also in every category
that every one of its top-level names matches: a zip of one `.exe` or of one `.app` is a
program, a tar of `.mp3`s is audio, a zip of a `.pdf` and a `.mp4` is only an archive. The top
level is the first path component of each entry, a directory's children folded into it, and a
lone wrapping folder that is not itself a bundle looked through, since `project-1.0/` is the
archive's own name and says nothing. The download window names the contents, up to six and a
count of the rest. Such an archive wears the icon and hue of the category its contents earned
it, in every list including Archives, since what is inside is what the thing is and the wrapper
is how it arrived; an archive whose contents say nothing more than its name keeps the archive
icon. All Tasks in the sidebar is a pyramid, the one shape that holds everything under it. Programs is where a program goes whatever wrapped it: the preset names the
installers and packages by extension, and the rule above brings the archives that hold one.

**The categories' hues are always on, unless asked otherwise.** "Always use colorful
categories", on to start with, has every category icon in the sidebar wear its hue all the
time; off, an icon is grey until its row is chosen or hovered and then its own hue. The state
filters above the categories -- All Tasks, Downloading, Unfinished, Completed -- keep the
chosen-or-hovered rule whatever the switch says: they are four, and always the same four, so
their hues are not a legend to anything. The funnel's menu follows the state filters' rule --
each status icon grey at rest, its status color while its row is chosen or hovered, All in snow
white -- since it is the same legend drawn a second time. Reorder keeps the switch's reading:
with the hues on, the rows keep them while they are dragged, and the row travelling under the
pointer wears its hue too; off, they are plain text with Other grey. Both readings of the
categories are right -- a column
of colors at rest is a legend to one eye and a rainbow to another -- so it is a switch, the
first row of Settings with something behind it, kept in config.json with the categories since
it is the user's. The list's type icons wear their hues either way, since there the color says
which bucket a file fell into. And either way the window's inactive grey wins: a background
window gives up its hues.

**The window's corners are macOS 27's, everywhere.** The radius is 17 points, measured rather
than chosen: a window captured without its shadow is transparent outside the corner, so the
first opaque pixel of each row traces the arc, and a circle fitted to it gives the number; a
Finder window traces the same arc, so it is the system's. The root draws itself rounded to it
and clips what it holds, which on macOS changes nothing, since the system clips the window to
the same curve, and on Windows and Linux is the only rounding the window gets, its frame being
the toolbar. A window that fills the screen, maximized or full screen, draws square corners,
as every system's own do.

**The window is blurred, and only the sidebar lets it show.** `WindowBackgroundAppearance::Blurred`
asks macOS for the blur behind a native window, but a native window is not transparent: Finder's
content is opaque and its sidebar is a *material*, mostly opaque with a hint of the desktop's
color bleeding through. So the list and the toolbar are solid Nord and the sidebar carries the
one alpha in the palette, high enough that what shows through is a tint rather than a picture.
A first cut with alpha on every surface read as a glass box, which is the look macOS 26 tried
and 27 stepped back from; the effect wanted is the older, quieter one, and it does not come from
turning transparency up.

**An inactive window gives up its hues.** The palette is built once per render from
`Window::is_window_active`, and when the window does not have the keyboard every accent, status
and selection color collapses to the muted grey while the greys stay. macOS drains a background
window the same way; a download manager sits in the background most of its life, and a wall of
color it is not being looked at is noise on the desktop.

The density is Zed's: a 13px UI face with everything in rems of it, 26px table rows, a 36px
toolbar. Not Zed's look -- the shapes, the glass and the palette are this application's.

## One thing, one window

A native application opens windows freely, and rdm does: the main window is the list and nothing
else, and anything about one item gets a window of its own. Double-clicking a row, or the name at
the right of the status bar, opens that download in a window that follows it live -- progress,
speed, remaining time, and the same pause, resume and remove actions -- and a second double-click
brings that window forward rather than opening another. Secondary windows keep the system titlebar: they are
documents, and the main window is the application, so closing the main window quits and closing a
secondary one closes only itself.

**A field's shortcuts take the system's modifier.** Select all, paste, copy and cut are
Command on macOS and Control on Windows and Linux, bound once in `text_input.rs` by platform; a
field bound to Command alone could not be pasted into anywhere else, which is how it shipped
first.

**Add Task looks, shows what it found, and asks one thing before it adds.** An address that
turns out to be a file is not added on the spot: the sheet shows its name and size, whether
the server serves ranges -- resumable, and splittable across connections -- or not, and,
when it does, a choice of `Auto` or `Fixed` with a field for the number, one to 256, offered
as the settings' default. Enter or Add again adds it, and the count travels with the row to
the engine and into the database, so a resume after a restart opens what was asked for.
Without ranges there is nothing to choose and the notice says so. A page's links and the page
itself take the settings' default. `Auto` is the engine's own judgement, in
[engine.md](engine.md).

**Everything else the engine can be asked is behind `More`.** Under the notice a word opens
the rest: the name to save under, the folder -- the system's picker, and the download folder
unless one is chosen -- other addresses of the same file apart by spaces, a checksum the
finished file must match, sha256, sha512 or md5 as hex with the length saying which, the part
of the file wanted as `start-end` in bytes, and a limit of the download's own. Each is checked
before anything is added and a wrong one is said under the field; each empty one is left to
the defaults. What was asked travels with the row into the database, and a resume after a
restart asks for the same. The download's window shows what was asked and lets the limit be
changed while it runs. The transfer settings, a section of their own, are the engine's
defaults for every new download and are listed in [release.md](release.md)'s neighbour,
[engine.md](engine.md); the two the engine takes live, concurrent downloads and the speed
limit, reach it as they are typed.

**Add Task reads the clipboard once and looks before it leaps.** Opening the sheet reads the
clipboard, and if what is there is under a thousand characters and reads as an address --
with a scheme, or without one and tried as https -- the field starts with it; anything else
leaves the field empty rather than guessing. A thousand is a hard ceiling: an address is never
longer, and a document that happens to be on the clipboard is not worth parsing. Enter or Add
does not queue the address; it has the engine look at it first. What is not an address is said
to be one, under the field. A file is queued and the sheet closes. A web page is said to be a
page, with a button to save it anyway and, under that, the files the page links to, each a row
that queues it when pressed and stays pressed; the sheet stays up so several can be taken. The
field itself scrolls under its cursor when the address is longer than the box, as every
native field does: wrapping would make a one-line field two, and an ellipsis would hide the
part being edited.

**A download's window says what was asked and takes one change.** Under the address and the
size: the folder, the connections, the mirrors, the checksum and the range where there were
any, the error while there is one, and a field for the download's own limit, applied on Enter
to the engine and kept on the row.

**Only a download gets a window; everything else is a sheet.** Settings and Add Task open as a
card over the dimmed list inside the main window. The distinction is whether the thing is worth
keeping beside the list while the list moves: a download is, and its window follows it live; a
form is filled in and dismissed, and a window for it is a window to find and close afterwards.
Add Task was tried as a sheet first and read better than the window it replaced, so Settings
followed.

**Settings is shaped for the many to come.** A rail down the left names the sections -- General,
Transfers, Appearance, About -- with a search field above it, and the chosen section's rows fill
the right. About is where the name in full lives, with the version, the build number, the
commit and the identifier, so what a build is can be read off it and told to someone. The card is a fixed size, so changing sections moves nothing. A search cuts across every
section and shows each match under its section's name, since a setting is looked for by what it
does, not by where it was filed; while a search is on, no section is lit. Escape in the field
closes the sheet, like Escape anywhere on a sheet with nothing to lose. Every setting belongs
to exactly one section; a setting with nothing behind it yet is shown as a value that cannot
be changed, marked `TODO` where it lives, rather than left out and rediscovered later. A row
can also be a word that does something -- `Check now` under `Latest build` -- with a note
beside it on how it last went, a choice of a few words with the chosen one lit -- `When a
build is found` -- or a field applied on Enter with a word on what it takes -- `Speed limit`
and `Connections` under Transfers, the first in kilobytes a second unless `m` or `g` says
otherwise and empty for none, the second `Auto` or a number up to 256; what a field says no to
is said under its row. Like a switch, a word and a choice do not take the keyboard; a field
does, being a field. A row that only means something
under another is shown only then: the choice under `Automatic updates` goes when the switch
is off. See [release.md](release.md) for what the three update rows do.

**A newer build is a card in the corner, not a sheet.** It sits over the list above the status
bar, at the right, and asks for nothing: the list stays usable, `Get` opens the file in the
browser, `Later` closes the card until the next build. Nothing modal, because nothing is
waiting on the answer. See [release.md](release.md).

The detail pane this replaces cost the list a fifth of its height to show one item's fields, and
was in the way whenever nothing was selected. The status bar it became is an editor's: one line
across the whole window, split where the sidebar is. Under the sidebar, the four actions -- add,
pause, resume, remove -- as icons that are always there and lit only when the selection allows
them. Under the list, from the left: a summary of the collection, how many and how fast; the
selected download as a link to its window; and at the corner, evenly spaced because they are
looked for together, the status funnel, the view switch and the Settings gear.

**The toolbar is two labelled buttons.** Add Task, and one button that says what the selection can
do next: Pause while it downloads, Resume while it is paused, queued or failed, Remove once it is
complete, with a cross rather than the trash can so the two removes read differently. Four
labelled buttons of which three were greyed at any moment spent the toolbar on saying no.

**An icon alone carries its name as a tooltip.** Every control drawn without a label -- the
corner icons, the action icons, the view segments, the funnel, the two switches on the custom
form, the icon picker -- names itself in a small label once the pointer has rested on it for
half a second, which is GPUI's own delay. A label that is always there is a toolbar; one that
appears when asked is how an icon stays an icon.

**Two hover languages, by whether there is a state to show.** A control that does one thing when
pressed -- the corner icons, the action icons, a menu row's icon -- brightens on hover and
nothing else: it has no pressed state, and a background would promise one. A control that stays
chosen -- a view segment, a sidebar filter, the funnel while a status is set -- keeps a
background for the state and hovers by brightening too. GPUI's svg carries its own color rather
than inheriting the text's, so the icon watches its button through a group to brighten. The toolbar is left with actions on the selection and nothing else, which is
what a toolbar is for; a view switch is not an action and was moved off it.

Two facts about GPUI decided the shape of the code. A secondary window's view holds an
`Entity<Rdm>` and observes it rather than copying anything, so there is one list and any number
of windows onto it. And a window cannot be opened from inside the click that asks for it: the new
window draws its first frame inside `open_window`, that frame reads the main view, and the main
view is still being updated by the click -- which panics. Opening is therefore deferred to after
the update, in `Rdm::open_download`.

## A sheet is modal, and a click outside closes it only while it is clean

**One rule for every sheet, and Escape follows it.** A sheet with nothing unsaved -- nothing
typed, nothing switched from how it came, or every change already applied and written --
closes from Escape or from a press outside it. Once it holds something unsaved, only its cross
closes it, and Escape in one of its fields does no more than a press outside would. Escape is
answered by the topmost sheet alone, since that is the one a press outside would reach; a
press on the guide's backdrop is a press on the guide, not on the form beneath it, and the
form's own press-outside is told so. Escape reaches the sheets because the window's root holds
the keyboard whenever nothing else does: a key travels the focus path and nowhere at all when
there is none, so the root takes focus back at every frame that finds it empty.

**A field holds the keyboard until something else is pressed, and a form's first field holds
it from the start.** GPUI moves focus only onto a focusable element that is pressed, never off
one on its own; and a sheet's backdrop occludes the window's root, which is the focusable thing
a press on the card would otherwise reach. So with one field on a sheet, no press anywhere on
the sheet took the keyboard from it. The backdrop under every sheet now answers a press that
no field claimed by dropping the focus, and the root takes it back at the next frame: a press
on the card, a button or a row leaves the field, as it does in a native window. A switch is
the exception -- an extension chip, the two toggles on the custom form, a switch in Settings --
since it is pressed in the middle of typing and the typing should carry on; its press claims
the keyboard the way a field's does, and the backdrop leaves the focus alone. Which field
starts with the keyboard is the sheet's decision, not the framework's -- nothing is focused
unless asked -- and the rule is by what the sheet is for: a form is opened to be filled in, so
Add Task, the custom category and a preset's list focus their first field; Settings is a place
to look around, so nothing in it takes the keyboard until pressed.

**The press that brings the window back does nothing else.** Coming from another
application, the first press asked for the window, not for the row, button or backdrop under
the pointer -- and a sheet closed by that press, or a row selected by it, was the wrong answer
to the wrong question. The platform marks that press, and an element drawn first in the root
swallows it in the capture phase before anything else sees it. An element, not a listener on
the root, because a listener fires only while its element is hovered and a sheet's backdrop
takes that away.


A sheet lies over a backdrop that takes every mouse event, so nothing behind it can be pressed
through it. Without that, a press on the sheet was also a press on whatever row lay under the
pointer -- two presets toggled twice became a double-click on a row and opened its window from
behind a sheet that was supposed to have the screen.

Clicking outside the sheet closes it, which is the habit every native dialog teaches, with one
line drawn: **only while there is nothing to lose.** A sheet with no input, or one whose fields
are still empty and whose choices are still their defaults, closes from a click outside. Once
something has been typed or chosen, a click outside does nothing, and the cross in the corner is
the way to discard it; Escape, a deliberate key, still cancels. Presets are exempt because they
act at once and leave nothing pending. Losing typed text to a stray click is the one thing a
dialog must never do, and it outranks the convenience the outside click buys.

## Categories are rules the user writes

The sidebar's categories are one kind of thing: a name, an icon and a regular expression over
the file name. Nine presets -- Videos, Audio, Images, Documents, eBooks, Code, Archives, Programs,
Disk Images -- are seeded into `config.json` on the first launch, followed by Other, which has no
pattern and takes whatever nothing else did.

**A sidebar label is in Title Case, and plural when it can be counted.** Title Case because
that is what macOS asks of sidebar items and what Finder's are; the plural because the row
names a set -- Videos, Documents, Disk Images -- and Finder's rows do the same. What cannot be
counted keeps its form: Audio, Code, Plain Text; and Other names the remainder, not a kind. A
preset renamed for this rule keeps reading under its old name from a config.json written
before, so nobody's list changes but the label.

**A file is in every category that matches it.** Two rules that both describe a file put it in
both, so there is nothing to order and no priority to drag; the row's icon is the first match's,
or the filtered category's own while one is filtered. The pattern is matched against the file
name and not the address: a name is what the user sees in the list, and a pattern over the whole
URL would catch hosts as often as files. The engine is `fancy-regex`, chosen over the plain
`regex` crate for look-around and backreferences -- "not a video" is `^(?!.*\.mp4$)` and cannot
be written without them -- at the cost of a backtracking engine, which for patterns over file
names is no cost anyone will measure.

The plus beside the Categories heading opens a sheet that shows one thing at a time. **It opens
on the presets**, each a switch: one press adds a preset to the sidebar, another removes it.
Under them, three ways on: Edit and Reorder, two words, and Add, a button.

**A preset is a list the application maintains and the user amends.** Each ships with its
extensions, and a release may add to them. The user's changes are kept apart from that list --
extensions added, and built-in ones removed -- and the list a preset runs with is the built-in
one less the removals, then the additions in the order they were typed. So a release that adds
an extension reaches every user who did not remove that one on purpose, and a file written by
an older build, which spelled presets as patterns, is read as the preset whenever its name is
a preset's and its pattern is a plain list of extensions: whatever that list had beyond the
built-in one is kept as additions, and nothing is marked removed, so the extensions added since
arrive as they do for everyone. The lists aim to be complete for what a download manager
meets -- every vendor's and every open format of the kind, and the newer ones like AVIF, JPEG
XL and Zstandard -- because a file that lands in Other for want of one extension is a list's
failure, not the user's. Presentations, Spreadsheets and Plain Text are presets of their own;
word processing and PDF stay under Documents.

Edit turns the chips into doors: a lit chip opens its list. The name stands alone above; the
line under it is every color the category could wear (see below); then every extension as a
chip that switches -- a built-in one off and back, struck through while off; an added one
simply dropped -- with a field that adds more; then the icon picker. Reset stands in the
card's bottom corner, a word with "Reset to default" as its tooltip, and only while anything
-- the list, the icon or the color -- differs from the preset as shipped; a press puts all
three back. Each change applies and is written as it is made, like the preset switches
themselves. A preset's icon and color are written to the file only when they are not the
preset's own, so one the user left alone follows the application's choice.

**Reorder happens in the sidebar, not in the sheet.** The sheet shrinks to one line with an
arrow pointing left, and every wash in the window but the sidebar's categories goes
dim -- the list, the toolbar and status bar, and the sidebar's own filters above the heading --
so the categories are the one lit thing. Each row grows a six-dot grip and is dragged onto the
row whose place it should take; the drop is applied and written as it lands. Other is neither
dragged nor a target, so it stays last and reads as the remainder. The face ends from Escape or
from a press anywhere that is neither the hint nor the categories: every drop is already
written, so there is nothing to lose, and a button to say "done" was one more thing to find. A
drag never ends it, since a drag begins on a row and the washes only answer presses on
themselves. The backdrop cannot cover the sidebar, so the sidebar dims its own filters with the
same wash, which closes like the rest, and the backdrop is cut around it.

**Add is a second level.** The custom form asks for a name, with a swatch beside it for the
color the icon will be lit in -- the next hue in the cycle to start with, so the swatch is
never blank -- one of sixteen Lucide icons, and a basic rule in two fields with a switch
between them: the extensions the category stands for, typed as `rs, py`, and text the name
contains, joined by AND or OR when both are filled -- AND by default, since a rule that names
both usually means both. After the text, two icon switches, both off to start: Match case,
since a name typed from memory is more often right in its letters than in its capitals, and
Ignore spaces, which lets any run of whitespace in the text match any run or none. The
application spells all of it as the one regular expression that actually runs, the extensions
always without regard to case, so nobody has to know what a regular expression is to make a
category. **Advanced** -- the one word, sized to itself so the rest of its line is not a way to
open it -- shares its line with Create while closed. Open, the pattern unfolds between them,
prefilled from the basic fields, for the rule they cannot say, and Create moves to the bottom
with the report. The empty field shows one worked example with a comment after it saying that
lookahead and lookbehind are supported; under the field, a sentence says the pattern is
matched against whole file names, and a line reports what the rule would do right now: the
engine's own error while it does not compile, or how many of the current downloads it catches.
The report belongs to Advanced alone; under the basic fields it read as noise, since those
always compile. The cross on the form, and on a preset's list, steps back to the presets, one
level up, rather than out; the cross on the presets closes. The icon list is sixteen glyphs;
the whole of Lucide would be a picker nobody finishes scrolling.

**The color line is the same everywhere.** The nine named hues as swatches, then a field that
fills the rest of the line for a color of the user's own, and after it a dot. The field reads
what this stack has constructors for and nothing more: hex in every common length, with or
without the hash, and the CSS functions `rgb()`, `rgba()`, `hsl()` and `hsla()`, alpha read and
dropped; its placeholder is one hex value, the way most people write one, and the rest of what
it reads sits behind a question mark after the field: "Read guidelines" on hover, a name like
any other icon's rather than a rule squeezed into a tooltip, and on a press the whole guide -- every shape with examples, and that alpha is dropped and names are
not read -- laid over the form with one button, OK, that takes it away and leaves the form as
it was. A placeholder that listed every shape was a sentence in a box meant for a word; a guide
unfolded under the line moved the form under the pointer; and a window of its own, tried next,
was somewhere else to look and something else to close. Over the form is where the eye already
is, and one press is all it costs. Nothing but OK closes it, since it was asked for. The dot previews what is typed as it is typed, and once the text reads as a color
the dot is a swatch like the nine: pressing it, or Enter in the field, chooses that color. The
writing stays with the category, as written, so the user can move between a named hue and
their own and back; on the new-category form it is kept only if the category is created. What
the field does not read -- names like `red`, `oklch()` -- it shows as an empty dot and ignores,
and that is the whole of the error handling: a color the user cannot see is one they will
retype.

Categories are read from `config.json` and written back when one is added, a preset switched
or its list changed, or the order changed; see [state.md](state.md). Editing or removing a custom one from the window is not built; the
file is the way, for now.

## What is deliberately not there yet

The rows come from the engine now (see [engine.md](engine.md)): Add Task hands the address to
it, the list is redrawn from its events a few times a second, and pause, resume and remove are
its commands with the row changed at once so a click never waits on a connection. What is not
there: the list is not persisted, so a restart forgets the rows while their partial files and
plans stay on disk for the store that does not exist yet; an address that is not a URL is
dropped without a word; the Settings sheet shows the download folder the engine writes to and
read-only values for the rest. Each is marked `TODO` where it lives.
