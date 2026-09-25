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

**The loop belongs to a terminal.** It is started where its output will be read, and it ends
with that terminal. An agent that starts one stops it before the reply ends, under the
workspace's rule that what an agent starts, an agent stops: a loop put in the background and
forgotten goes on killing and reopening the window for as long as the machine is up, long after
the session that started it is gone, and the person whose window it is has nothing to connect
it to.

**A link failure of missing `_anon.<hash>.llvm` symbols is the build tree, not the code.** It
reads as a real error -- `ld: symbol(s) not found for architecture arm64`, naming drop glue in
whatever crate happened to be compiled last, `rustls` one time and `hickory_resolver` the next --
and `cargo check` and `cargo test` both pass while it happens, because neither links the binary
`cargo run` does. It is stale incremental state, and `cargo clean -p rdm` clears it in seconds;
the dependencies are untouched, so the rebuild that follows is one crate.

**What produces it is two cargo invocations in one `target/`**: the loop's `cargo run` and a
`cargo test` in another terminal, which is exactly what the "Blocking waiting for file lock on
build directory" line in the loop's output is announcing. It has happened twice. The way not to
meet it is to stop the loop before running the suite, which costs one restart; the way out when
it does happen is the clean above, and reaching for the code first is time spent on a defect that
is not there.

**One loop per checkout.** Two of them share `target/` and cargo's lock on it, so the second
sits at `Blocking waiting for file lock on build directory` and the two windows take turns
reopening. `pgrep -f 'watchexec --restart'` says whether one is already up; stopping it is
killing that process and the `mise run dev` above it. `RDM_PID` is for the moment a restart
leaves the old process up, not a way to live with two loops.

**It watches `src/` and the project root, and nothing else.** Everything else in the tree is
outside it, and what that costs differs by directory. A change under `spec/` restarts nothing,
so waiting for the window to come back after one is waiting for an event nothing set in motion.
`locales/` is `include_str!`, so a translation needs a rebuild the loop will not start on its
own: touch something under `src/`, or restart the loop. `assets/` needs no rebuild at all --
rust-embed only embeds when `debug_assertions` are off, so a debug build reads the icons from
disk and a fetch reaches the window without the compiler.

**The window it opens is not the installed one.** A debug build's state, config and database sit
under the `.dev` name, so the loop's window and an installed `.app` can both be up at once,
remembering different things; see [state.md](state.md). That is the usual reason more than one
build is up, and why `shot` and `ax` want `RDM_PID` when it is.

## Driving the window without the mouse

A GPUI window is not a web view, so nothing made for a browser reaches it. The tools below
stand in for a browser's, each answering one question, and none of them moves the
pointer or takes the keyboard -- the one afternoon that simulated clicks did, the user lost
their mouse to it, and that is the rule this section exists to keep.

| Question                              | Tool                              | Touches the screen |
| ------------------------------------- | --------------------------------- | ------------------ |
| What is the state, and change it      | `mise run ctl <command>`          | no                 |
| What is drawn, and which code drew it | `mise run ctl tree [match]`       | no                 |
| Where is each element, and press it   | `mise run ax tree` / `press`      | no                 |
| How wide is the window, and resize it | `mise run ax size <w> <h>`        | no                 |
| Does a click do the right thing       | `cargo test`, headless            | no window at all   |
| What does it look like                | `mise run shot [path] [title]`    | reads pixels only  |
| What does a notice window look like   | `mise run shot <path> --floating` | reads pixels only  |

**`ctl` is the debug build's control socket.** `src/ctl.rs` listens on a Unix socket under
`target/` in debug builds only -- on Unix only, since the standard library has no such socket
on Windows, where a debug build simply has none -- one line in and the application's state out as JSON -- filter,
sort, view, selection, open windows, how many rows the list holds and shows, every download -- with commands for what the toolbar,
sidebar, chips, headers and rows do. It is the analogue of the Tauri MCP bridge the workspace
uses for its webview app, kept to a socket and a Python client because that is all the job
needs. The socket lives in `target/` so it is per checkout and gone with `cargo clean`.

**`ctl tree` is the inspector, and the first thing to reach for.** It is GPUI's own dump of the
accessibility tree it last built for each window -- `Window::debug_a11y_tree_json`, richer in a
debug build -- reshaped into the tree it describes and printed as an outline: a node a line, with
its role, label, value, id and the source line that constructed it, under a header giving the
node count, the tab stops and the viewport. A match narrows it to the windows whose title holds
it, or else to the subtrees whose id or label it is; `--json` gives the nested form. It answers
in a fraction of a second, and that is why it comes first. What a sheet holds, whether a field
has what was typed, which component drew a row: each is a question with a text answer, and a
screenshot gives that answer slowest and least exactly. The user asked for exactly this -- a
channel like the DevTools a web page has, with screenshots kept for when nothing else will do.

Two limits come with the source. **GPUI builds the tree only once something has asked the window
for it**, and on macOS the adapter then stays on for the rest of the process. So the reply names
the windows nobody has asked -- a download's window opened after the rest were woken is one --
and while any is asleep the client wakes them with `ax windows` and asks again; a fresh `dev`
restart or a new window costs one extra round. **Only an element with both an id and a role is a
node.** A plain string child has neither, so text a tool or VoiceOver should read is written
`text!(...)`: a `Label` whose id is its source location, or `text!(id = ..., ...)` where one
call site draws several. A container that should gather what is inside it takes an id, a role
and a label, as the New Task card does with `Dialog`. `TextInput` is a `TextInput` node with its
content as the value, so a field's text is read rather than inferred from a screenshot. The dump
carries no bounds, so layout is read from `ax tree`, which ends each line with the element's
frame in points from the window's top-left.

`ctl state` covers the sheets as well as the list: `add` is the New Task sheet's fields and what
looking at the address found, and which of its two screens is up. `ctl look <address>` types an
address into the open sheet and looks at it as Enter on the first screen would, which reaches the
second screen, or the first screen's question about a page, without the keyboard and adds nothing.
`ctl slide limit <0-1>` and `ctl slide range <0|1> <0-1>` move a slider as a drag would, through the
call the drag makes. The sheet itself is opened with `ax press "Add Task"`.

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
and it is the last of these to reach for: when neither the source nor `ctl tree` and `ax tree`
can answer a question about what is on screen -- colour, blur, whether glyphs drew at all -- and
never to confirm that a change typed is a change made.

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

**A headless test that names a value the platform chooses passes here and fails there.** The
tests run on all four systems and the same assertion is not the same assertion on each. The one
that caught this asserted on `Chrome on Linux` being among the user agents offered -- and
`Agent::offered` leaves out the disguise that would be this machine telling the truth, so on
Linux that option is precisely the one that is not there. Green on a Mac, red on both Linux
runners, and nothing local to see.

So **a test asserts on the rule, not on a value the rule happens to produce here**: that the
offered set excludes this system's own, rather than that it contains a named one. Where a test
really does want a platform's answer it says which platform, with a `cfg`, and then it is a test
of that arm rather than a test that travels badly.

So **anything behind a `cfg` carries the same `cfg` on whatever it needs**: the import beside the
function, the constant beside its one reader, the method beside its callers. Written that way
there is nothing to notice later. Written the other way it is invisible from here and shows up
only in a log nobody opens.

The nightly's four builds are the only place the whole picture exists, and it does not fail on a
warning, so the warnings sit in the run's annotations while the run stays green. Reading them is
part of reading the build. `cargo check --release` catches the profile half from here; the
platform half cannot be cross-checked from a Mac, because both crypto backends compile C and
neither has a Windows or Linux toolchain to compile it with. Docker covers Linux.
