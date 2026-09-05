# Working on the window

## The rebuild loop

`mise run dev` is still's screen loop applied here: watchexec restarts `cargo run` on any change
to a `.rs` or `.toml` file, so the window closes, rebuilds and reopens on its own. The
`--project-origin .` flag and the extension filter are explained where they are set, in
`mise.toml`; the short version is that watchexec would otherwise walk up to the workspace's
`.gitignore`, which ignores this whole directory.

## Driving the window without the mouse

A GPUI window is not a web view, so nothing made for a browser reaches it, and there is no
inspector to open. Four tools stand in, each answering one question, and none of them moves the
pointer or takes the keyboard -- the one afternoon that simulated clicks did, the user lost
their mouse to it, and that is the rule this section exists to keep.

| Question                                    | Tool                          | Touches the screen |
| ------------------------------------------- | ----------------------------- | ------------------ |
| What is the state, and change it            | `mise run ctl <command>`      | no                 |
| What does the window contain, and press it  | `mise run ax tree` / `press`  | no                 |
| Does a click do the right thing             | `cargo test`, headless        | no window at all   |
| What does it look like                      | `mise run shot [path] [title]` | reads pixels only |

**`ctl` is the debug build's control socket.** `src/ctl.rs` listens on a Unix socket under
`target/` in debug builds only, one line in and the application's state out as JSON -- filter,
sort, view, selection, open windows, every download -- with commands for what the toolbar,
sidebar, chips, headers and rows do. It is the analogue of the Tauri MCP bridge the workspace
uses for its webview app, kept to a socket and a Python client because that is all the job
needs. The socket lives in `target/` so it is per checkout and gone with `cargo clean`.

**`ax` is the accessibility tree, which macOS already has.** Every interactive element carries
a role and a label -- `Button "Pause"`, `RadioButton "Filter: Video"`, `CheckBox "Completed 2"`,
`ListItem "rust-book.pdf"` -- so the tree a screen reader sees is a structural snapshot an agent
can read, and `AXPress` performs an element's own action through the same channel. This is the
one of the four that is also a feature: it is what makes the application usable with VoiceOver.
Two facts about it are not obvious. AccessKit builds the tree lazily, so the first query only
switches it on and the elements arrive with the next frame; `ax` reads twice for that reason.
And element ids must be unique within a frame: a chip, a sidebar row and a header cell all
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
