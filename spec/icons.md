# Icons

## Lucide, tinted at the call

Icons are [Lucide](https://lucide.dev), the set Zed itself draws with, under its ISC licence,
which travels with the files. One set, so every glyph shares a stroke weight and an optical
size; the toolbar, the rows and the status marks all read as one hand.

Every icon takes its color as an argument. GPUI paints an svg only when `text_color` is set
on the svg element itself and never inherits it from the text around it, so an untinted icon
is a blank square that raises no error -- which is how the first build looked. Making the
color a parameter of `ui::icon::icon` turns that silent failure into a type error.

## A status is ringed as a mark and bare as a legend

The same five statuses are drawn in two places, and never the same way. Down the list's Status
column they are **ringed** -- `circle-check`, `circle-x`, `circle-pause`, `circle-arrow-down`,
and the clock, which is a circle drawn as a face. In the sidebar and in the funnel's menu they
are **bare**: `check`, `x`, `pause`, `arrow-down`, `hourglass`.

The reason is what each one has to do. A mark in the column is read down an edge, beside a word,
at three points across; at that size a bare tick and a bare cross are two strokes each, and the
column reads as scratches of different sizes. The ring gives every mark the same outline, so the
eye finds the column before it reads any of it. A legend is the opposite: one glyph a line, with
its word beside it and room around it, and there a ring is a box drawn around a picture that did
not need one. The split also says which is which -- a ring in this window is a row's own state,
never something to filter by.

`Icon::for_status` is the ringed table and `Icon::for_status_filter` the bare one, both in
`src/ui/icon.rs`, and a test holds them apart. Where the sidebar names the same thing the menu
does -- Downloading, Completed -- `Icon::for_filter` returns the same glyph as the bare table,
because they are one legend drawn in two places. Two of the sidebar's four name something no
status does and are the exceptions: All Tasks is a **pyramid**, a shape rather than a mark, the
one thing that holds everything under it; and Unfinished is a **dashed circle**, which is not a
ring around anything but an outline not yet closed, which is what unfinished looks like.

## Declared in the source, fetched by a task

The files are not committed. `assets/lucide/` is ignored, and `mise run icons` fetches whatever
the source names that is not yet on disk. The directory is named for the set, against the
usual rule that vendor names stay at the binding edge, because its contents are the set's files
unchanged under the set's licence: vendored content is named for its vendor, the way a
`vendor/` directory is. The declaration is the `Icon::path` match in `src/ui/icon.rs`: every
`"lucide/<name>.svg"` string there is one file to fetch, and the task
reads the list out of the source rather than from a manifest beside it, because a manifest would
be a second copy of the same list and would drift from the first.

Two consequences, both intended:

- Adding an icon is one enum variant and one match arm, then any build task. `check`, `lint`,
  `test` and `dev` all depend on `icons`, so a fresh clone needs no step it has to know about,
  and the fetch skips files already present so it costs nothing afterwards.
- A bare `cargo build` does not run the fetch. rust-embed embeds what is on disk at compile
  time, so a build outside mise on a clone with no icons produces a binary with none, and no
  error. The workspace's tasks are the supported way to build; this is one more reason.

## Pinned to a release, not to `main`

The task fetches from a Lucide release tag named in the script, not from the default branch. An
icon is vendored content: a redraw upstream would otherwise change the binary on the next clone
with nothing in this repository saying so. Raising the tag is a deliberate edit with a diff
behind it, the same rule the workspace applies to every tool version.
