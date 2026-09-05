# Working on the window

## The rebuild loop

`mise run dev` is still's screen loop applied here: watchexec restarts `cargo run` on any change
to a `.rs` or `.toml` file, so the window closes, rebuilds and reopens on its own. The
`--project-origin .` flag and the extension filter are explained where they are set, in
`mise.toml`; the short version is that watchexec would otherwise walk up to the workspace's
`.gitignore`, which ignores this whole directory.

## Seeing the window

A GPUI window is not a web view. Chrome DevTools MCP reaches a browser tab and the workspace's
Tauri MCP server reaches a webview; neither can see this. `mise run shot [path]` is the feedback
loop instead: a Swift script asks CoreGraphics for the window owned by the rdm process and hands
its id to `screencapture -l`, which captures that one window and nothing else. The result is a
file an agent can read back.

It exists because two defects were invisible any other way -- a build that drew no text and a
build that drew no icons, neither of which produced an error -- and each was diagnosed from one
capture. It is used under the workspace's rule for checking one's own work: when a question
about the running window cannot be answered from the source, not to confirm that a change
typed is a change made.

## Every build task fetches the icons first

`check`, `lint`, `test` and `dev` depend on `icons`, so the assets the binary embeds are
present before cargo runs. See [icons.md](icons.md) for why they are fetched rather than
committed.
