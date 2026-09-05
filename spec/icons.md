# Icons

## Lucide, tinted at the call

Icons are [Lucide](https://lucide.dev), the set Zed itself draws with, under its ISC licence,
which travels with the files. One set, so every glyph shares a stroke weight and an optical
size; the toolbar, the rows and the status marks all read as one hand.

Every icon takes its color as an argument. GPUI paints an svg only when `text_color` is set
on the svg element itself and never inherits it from the text around it, so an untinted icon
is a blank square that raises no error -- which is how the first build looked. Making the
color a parameter of `ui::icon::icon` turns that silent failure into a type error.

## Declared in the source, fetched by a task

The files are not committed. `assets/icons/` is ignored, and `mise run icons` fetches whatever
the source names that is not yet on disk. The declaration is the `Icon::path` match in
`src/ui/icon.rs`: every `"icons/<name>.svg"` string there is one file to fetch, and the task
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
