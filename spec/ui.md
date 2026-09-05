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

## Three views, Detailed by default

The list draws three ways and a segmented control at the toolbar's right end picks one:

| View     | A row is                                                        | For                        |
| -------- | --------------------------------------------------------------- | -------------------------- |
| Detailed | a table row: type, name, size, progress with percent, speed, status | the default; shows it all |
| Compact  | one 22px line: type, name, a short bar, size, a status mark     | a long queue               |
| Grid     | a card with a large type icon                                   | scanning by type           |

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
under a left-aligned title. A handle sits at each column's left edge and
drags that column: the table is anchored at its right and the name column absorbs the rest, so a
column's left edge is the one that can move, and a boundary that follows the pointer is what a
drag means -- the first cut put the handle on the right and read as reversed. The widths live on
the view, and every row spends the same twelve points on the handle's gap so cells stay under
their titles. A drag is clamped at both ends: a column no narrower than fits its numbers, and no
wider than leaves the name column its floor, because past that the row runs out of the window. A drag is tracked on the window root, not on the handle, because the pointer leaves
the handle the moment it moves; a move with the button up ends it, since a release outside the
window is never seen and would otherwise leave the next pointer movement resizing on its own. A funnel at the
status bar's corner opens a menu of statuses that cuts within whatever the sidebar selected --
one at a time, with All to clear it -- and the funnel stays lit, naming the status, while one is
chosen. The statuses were a row of chips above the list first, then a row in the status bar, and
ended in a menu: a filter is consulted far less often than the rows are read, so it earns an icon
and a click, not a strip of the window. The sidebar answers "which downloads", the menu answers
"in what state", and neither duplicates the other.

The view, sort and chip are not yet remembered across launches; that waits on there being any
persistence at all.

## Colour: Nord, through glass, drained when inactive

The palette is [Nord](https://www.nordtheme.com), in `src/ui/theme.rs`, and every name there
says what a colour is for -- `panel`, `border`, `muted`, `selection` -- never what it looks like.
Status is the exception and is colour-coded on purpose: aurora green complete, orange paused,
red failed, frost blue for downloading, grey queued, because a column of status words reads
slower than a column of colours. Selection and hover are Nord's polar-night greys; the frost blue
is reserved for progress that is moving.

**The window is blurred, and only the sidebar lets it show.** `WindowBackgroundAppearance::Blurred`
asks macOS for the blur behind a native window, but a native window is not transparent: Finder's
content is opaque and its sidebar is a *material*, mostly opaque with a hint of the desktop's
colour bleeding through. So the list and the toolbar are solid Nord and the sidebar carries the
one alpha in the palette, high enough that what shows through is a tint rather than a picture.
A first cut with alpha on every surface read as a glass box, which is the look macOS 26 tried
and 27 stepped back from; the effect wanted is the older, quieter one, and it does not come from
turning transparency up.

**An inactive window gives up its hues.** The palette is built once per render from
`Window::is_window_active`, and when the window does not have the keyboard every accent, status
and selection colour collapses to the muted grey while the greys stay. macOS drains a background
window the same way; a download manager sits in the background most of its life, and a wall of
colour it is not being looked at is noise on the desktop.

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

**Only a download gets a window; everything else is a sheet.** Settings and Add URL open as a
card over the dimmed list inside the main window. The distinction is whether the thing is worth
keeping beside the list while the list moves: a download is, and its window follows it live; a
form is filled in and dismissed, and a window for it is a window to find and close afterwards.
Add URL was tried as a sheet first and read better than the window it replaced, so Settings
followed.

The detail pane this replaces cost the list a fifth of its height to show one item's fields, and
was in the way whenever nothing was selected. The status bar it became is an editor's: one line
across the whole window, split where the sidebar is. Under the sidebar, the four actions -- add,
pause, resume, remove -- as icons that are always there and lit only when the selection allows
them. Under the list, from the left: a summary of the collection, how many and how fast; the
selected download as a link to its window; and at the corner, evenly spaced because they are
looked for together, the status funnel, the view switch and the Settings gear.

**The toolbar is two labelled buttons.** Add URL, and one button that says what the selection can
do next: Pause while it downloads, Resume while it is paused, queued or failed, Remove once it is
complete, with a cross rather than the trash can so the two removes read differently. Four
labelled buttons of which three were greyed at any moment spent the toolbar on saying no.

**Two hover languages, by whether there is a state to show.** A control that does one thing when
pressed -- the corner icons, the action icons, a menu row's icon -- brightens on hover and
nothing else: it has no pressed state, and a background would promise one. A control that stays
chosen -- a view segment, a sidebar filter, the funnel while a status is set -- keeps a
background for the state and hovers by brightening too. GPUI's svg carries its own colour rather
than inheriting the text's, so the icon watches its button through a group to brighten. The toolbar is left with actions on the selection and nothing else, which is
what a toolbar is for; a view switch is not an action and was moved off it.

Two facts about GPUI decided the shape of the code. A secondary window's view holds an
`Entity<Rdm>` and observes it rather than copying anything, so there is one list and any number
of windows onto it. And a window cannot be opened from inside the click that asks for it: the new
window draws its first frame inside `open_window`, that frame reads the main view, and the main
view is still being updated by the click -- which panics. Opening is therefore deferred to after
the update, in `Rdm::open_download`.

## A sheet is modal, and a click outside closes it only while it is clean

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
the file name. Nine presets -- Video, Audio, Images, Documents, Ebooks, Code, Archives, Programs,
Disk images -- are seeded into `config.json` on the first launch, followed by Other, which has no
pattern and takes whatever nothing else did.

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
an older build, which spelled presets as patterns, is read as the preset whenever its name and
pattern are one. Edit turns the chips into doors: a lit chip opens its list, where every
extension is a chip that switches -- a built-in one off and back, struck through while off; an
added one simply dropped -- with a field that adds more and Reset while anything has been
changed. Each change applies and is written as it is made, like the preset switches themselves.

**Reorder happens in the sidebar, not in the sheet.** The sheet shrinks to one line with an
arrow pointing left and a check, and every wash in the window but the sidebar's categories goes
dim -- the list, the toolbar and status bar, and the sidebar's own filters above the heading --
so the categories are the one lit thing. Each row grows a six-dot grip and is dragged onto the
row whose place it should take; the drop is applied and written as it lands. Other is neither
dragged nor a target, so it stays last and reads as the remainder. This face ends only from its
check or Escape: a press on the dim wash does nothing, because outside the card is where the
work is, and the one press that reached the sidebar by mistake closed the face under the
pointer. The backdrop cannot cover the sidebar, so the sidebar dims its own filters with the
same wash and the backdrop is cut around it.

**Add is a second level.** The custom form asks for a name, one of fourteen Lucide icons, and a
basic rule in two fields with a switch between them: the extensions the category stands for,
typed as `rs, py`, and text the name contains, joined by AND or OR when both are filled -- AND
by default, since a rule that names both usually means both. After the text, two icon switches:
one ignores case, the other lets any run of whitespace in the text match any run or none. The
application spells all of it as the one regular expression that actually runs, the extensions
always without regard to case, so nobody has to know what a regular expression is to make a
category. **Advanced** -- the one word -- unfolds that pattern for editing, prefilled from the
basic fields, for the rule they cannot say. The empty field shows one worked example with a
comment after it saying that lookahead and lookbehind are supported; under the field, a
sentence says the pattern is matched against whole file names, and a line reports what the
rule would do right now: the engine's own error while it does not compile, or how many of the
current downloads it catches. The report belongs to Advanced alone; under the basic fields it
read as noise, since those always compile. The cross on the form, and on a preset's list, steps
back to the presets, one level up, rather than out; the cross on the presets closes. The icon
list is fourteen glyphs; the whole of Lucide would be a picker nobody finishes scrolling.

Categories are read from `config.json` and written back when one is added, a preset switched
or its list changed, or the order changed; see [state.md](state.md). Editing or removing a custom one from the window is not built; the
file is the way, for now.

## What is deliberately not there yet

The rows are sample data advanced by a timer so the list moves; there is no transfer engine,
no persistence, and the Settings window is labels with nothing behind them. Add URL is a sheet with one field over the dimmed list. It queues what is typed
under the address's last path segment; the engine will resolve the real name and size. Enter
adds, Escape or a click outside closes. The mock
rows that download loop back to zero when they fill, so there is always movement to look at. Each is marked `TODO` where it lives. The window's job so far is to be the thing those are
built behind.
