# The window

## What it is modelled on

A download manager's window has a settled shape, and rdm keeps to it rather than inventing one:
a sidebar of filters on the left, the list in the middle, actions along the top and the selected
item's detail along the bottom. Neat Download Manager is the reference for what goes where; Zed is the reference for how tightly. The
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

**The table's header sorts.** Clicking a column title orders by it; clicking it again flips the
direction, marked by a chevron. The default order is arrival, which has no column and so shows
no chevron. Above the header a row of status chips cuts within whatever the sidebar selected --
one at a time, and clicking the lit chip clears it, so there is no "all" chip to keep in step.
The sidebar answers "which downloads", the chips answer "in what state", and neither duplicates
the other.

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

## What is deliberately not there yet

The rows are sample data advanced by a timer so the list moves; there is no transfer engine,
no persistence, and Add URL inserts a placeholder because there is no text input to type into.
Each is marked `TODO` where it lives. The window's job so far is to be the thing those are
built behind.
