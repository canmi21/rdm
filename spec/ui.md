# The window

## What it is modelled on

A download manager's window has a settled shape, and rdm keeps to it rather than inventing one:
a sidebar of filters on the left, the list in the middle, actions along the top and the selected
item's detail along the bottom. Neat Download Manager is the reference for what goes where. The
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

| View     | A row is                                             | For                       |
| -------- | ---------------------------------------------------- | ------------------------- |
| Detailed | type badge, name, progress bar, size, status         | the default; shows it all |
| Compact  | one line with fixed columns for progress, size, status | a long queue            |
| Grid     | a card with a large type icon                        | scanning by type          |

Detailed is the default because it is the only one that shows progress, speed and size at once,
which is what a download manager is open for. The other two trade that for density or for a
glance. The choice is not yet remembered across launches; that waits on there being any
persistence at all.

## Colour

One dark palette, in `src/ui/theme.rs`, named by role and never by hue: `panel`, `border`,
`muted`, `selection`. Status is the exception and is colour-coded on purpose -- green complete,
orange paused, red failed, the accent blue for downloading, grey for queued -- because a column
of status words reads slower than a column of colours.

Selection and hover are neutral greys. An earlier blue-grey selection read as a colour from
nowhere, since nothing else in the window was that hue; the accent blue is reserved for
progress that is actually moving.

## What is deliberately not there yet

The rows are sample data advanced by a timer so the list moves; there is no transfer engine,
no persistence, and Add URL inserts a placeholder because there is no text input to type into.
Each is marked `TODO` where it lives. The window's job so far is to be the thing those are
built behind.
