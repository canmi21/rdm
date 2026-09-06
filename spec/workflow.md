# Working on the window

## The rebuild loop

`mise run dev` is still's screen loop applied here: watchexec restarts `cargo run` on any change
to a `.rs` or `.toml` file under `src/` or in this directory itself, so the window closes,
rebuilds and reopens on its own. The `--project-origin .` flag, the watch roots and the extension
filter are explained where they are set, in `mise.toml`; the short version is that watchexec
would otherwise walk up to the workspace's `.gitignore`, which ignores this whole directory, and
that being ignored spares `target/` the filter but not the watch, which is why the roots are
named rather than left to default to the whole tree.

**A restart with no source change is the watcher, not the code.** Cargo says which it was:
`Finished ... in 0.2s` above the relaunch means nothing recompiled, so nothing under the filter
had really changed and the event came from the watcher itself -- a dropped-event rescan, which
watchexec reads as a change. `[Command exited with 101]` in the same log is the other kind
entirely: the application panicked and left, and the next line up says why.

## Driving the window without the mouse

A GPUI window is not a web view, so nothing made for a browser reaches it, and there is no
inspector to open. Four tools stand in, each answering one question, and none of them moves the
pointer or takes the keyboard -- the one afternoon that simulated clicks did, the user lost
their mouse to it, and that is the rule this section exists to keep.

| Question                                    | Tool                          | Touches the screen |
| ------------------------------------------- | ----------------------------- | ------------------ |
| What is the state, and change it            | `mise run ctl <command>`      | no                 |
| What does the window contain, and press it  | `mise run ax tree` / `press`  | no                 |
| How wide is the window, and resize it       | `mise run ax size <w> <h>`    | no                 |
| Does a click do the right thing             | `cargo test`, headless        | no window at all   |
| What does it look like                      | `mise run shot [path] [title]` | reads pixels only |
| What does a notice window look like         | `mise run shot <path> --floating` | reads pixels only |

**`ctl` is the debug build's control socket.** `src/ctl.rs` listens on a Unix socket under
`target/` in debug builds only -- on Unix only, since the standard library has no such socket
on Windows, where a debug build simply has none -- one line in and the application's state out as JSON -- filter,
sort, view, selection, open windows, every download -- with commands for what the toolbar,
sidebar, chips, headers and rows do. It is the analogue of the Tauri MCP bridge the workspace
uses for its webview app, kept to a socket and a Python client because that is all the job
needs. The socket lives in `target/` so it is per checkout and gone with `cargo clean`.

`ctl say <finished|failed|queue|update> [text]` makes a notice happen on demand, which is
otherwise a matter of waiting for a download to end; it says the words the real call sites say,
since a harness that says something else verifies something else. A notice sent to a window of
its own is on a layer above the ordinary windows and so is invisible to `shot`, which takes the
application's window: `shot <path> --floating` takes that one.

`ctl drag <column> <points>` is there for the one gesture nothing else can reach: a drag. A
handle has no action of its own -- it answers a press and then the pointer -- so the
accessibility tree cannot perform it and the rule against moving the pointer forbids the
obvious alternative. The command presses, walks the pointer in ten steps and releases, through
the same three functions a real drag calls, so what it exercises is the drag and not a copy of
it; ten steps rather than one jump because the bug it was written for only appeared on the
second move. Negative points widen the column, since the handle is its left edge. `state`
reports the table beside everything else: the widths asked for, the widths there is room to
draw, and what the name column is left. See [ui.md](ui.md).

**`ax` is the accessibility tree, which macOS already has.** Every interactive element carries
a role and a label -- `Button "Pause"`, `RadioButton "Filter: Videos"`, `CheckBox "Completed 2"`,
`ListItem "rust-book.pdf"` -- so the tree a screen reader sees is a structural snapshot an agent
can read, and `AXPress` performs an element's own action through the same channel. This is the
one of the four that is also a feature: it is what makes the application usable with VoiceOver.
Two facts about it are not obvious. AccessKit builds the tree lazily, so the first query only
switches it on and the elements arrive with the next frame; `ax` reads twice for that reason.
`ax size` sets the window's size through the same attribute a drag on its edge ends at, which
is how the system's own minimum size gets a say: ask for narrower than the window allows and
what comes back is what it allowed, which is the only way to check that a minimum is really
enforced and not merely declared. `RDM_PID` picks the process when more than one build is up,
which is often -- a `dev` restart leaves the old one a moment, and a second checkout is a
second window. And element ids must be unique within a frame: a chip, a sidebar row and a header cell all
called "Completed" collided, which surfaced as a click whose down and up landed on different
state in a headless test and as a duplicate-node panic in the accessibility tree, in the same
hour.

**The headless tests are GPUI's own test platform.** `gpui`'s `test-support` feature draws
the window into no screen; `debug_selector` names the elements a test wants to click, and
`simulate_click` at their drawn bounds exercises exactly the path a real click takes. They
run under `cargo test` with no display, which makes them the check that runs everywhere; the
other three need the application up. What they cannot see is pixels, fonts or blur.

**`shot` is for the eye.** A Swift script asks CoreGraphics for the window owned by the rdm
process and hands its id to `screencapture -l`, which captures that one window and nothing
else; a title picks one of several windows, the frontmost otherwise. It exists because two
defects -- a build that drew no text and one that drew no icons -- were invisible any other way,
and it is used under the workspace's rule for checking one's own work: when the source cannot
answer a question about what is on screen, not to confirm that a change typed is a change made.

## Every build task fetches the icons first

`check`, `lint`, `test` and `dev` depend on `icons`, so the assets the binary embeds are
present before cargo runs. See [icons.md](icons.md) for why they are fetched rather than
committed.

## The local check sees one system and one profile

`mise run check` is macOS in a debug build, and that is two blind spots rather than one. An
import or a constant used only inside a `#[cfg(target_os = "macos")]` arm is used here and dead
everywhere else; a method reached only from the control socket is reached here and dead in every
release build, since the socket is `#[cfg(all(debug_assertions, unix))]`. Five such warnings had
been accumulating in the nightly's logs unread, four of them from the day the code was written.

So **anything behind a `cfg` carries the same `cfg` on whatever it needs**: the import beside the
function, the constant beside its one reader, the method beside its callers. Written that way
there is nothing to notice later. Written the other way it is invisible from here and shows up
only in a log nobody opens.

The nightly's four builds are the only place the whole picture exists, and it does not fail on a
warning, so the warnings sit in the run's annotations while the run stays green. Reading them is
part of reading the build. `cargo check --release` catches the profile half from here; the
platform half cannot be cross-checked from a Mac, because both crypto backends compile C and
neither has a Windows or Linux toolchain to compile it with. Docker covers Linux.
