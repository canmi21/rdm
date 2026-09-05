# Packaging

## The icon is drawn on Apple's grid, not edge to edge

`assets/icon/rdm.png` is the artwork as Icon Composer exported it -- 1024 square, the system's
rounded corners already cut, no margin -- re-encoded from 16 to 8 bits a channel, which halves
it to under the megabyte jj will snapshot without being asked and loses nothing an icon shows. macOS does not draw an icon that way. Its template is a
1024 canvas on which the icon *shape* is 824 wide and centred, leaving 100 of transparent margin a
side; every icon in the Dock and Finder is laid out on that grid, and one drawn to the canvas's
edge sits visibly larger than its neighbours. The corner radius the template expects is 185.4 on
the 824 shape, which is what the artwork's corners come to once scaled, so the artwork is scaled
and padded and not re-cut.

`mise run icon` does that with CoreGraphics and renders each size the `.icns` format wants from
the 1024 result, then calls `iconutil`. It runs from the artwork every time, so the artwork is the
only thing committed and the rendered set lives under `target/`.

## The .app is assembled by a task, not a tool

`mise run bundle` builds the release binary and lays out `target/bundle/rdm.app`: the binary under
`Contents/MacOS`, the icon under `Contents/Resources`, an `Info.plist` naming both, and an ad hoc
signature, without which recent macOS refuses to launch even a local binary. The bundle
identifier is read out of `src/identity.rs` so it has one home; the version out of `Cargo.toml`
for the same reason. A packaging crate was considered and is not worth its dependencies for a
folder with four files in it; the task is forty lines and says exactly what the bundle contains.

Bundling is not part of `verify`. It is a step taken when there is something to look at or hand
over, and a release build is minutes where a check is seconds.

## Two profiles, two questions

**Development asks how fast a change can be looked at.** Our own code is built at `opt-level = 0`
so it compiles in seconds and steps cleanly in a debugger; the dependencies are built at 3, since
they change rarely and GPUI unoptimised is too slow to judge a layout in. The split is Cargo's
`[profile.dev.package."*"]`, which names every crate but ours.

**Codegen units are not the lever they look like.** The count sets how many pieces LLVM optimises
in parallel; more pieces is more parallelism up to the machine's cores and less inlining across
them. At `opt-level = 0` LLVM optimises nothing, so the count changes neither the speed of the
build nor of the code, and 256 -- the default, written down so nobody wonders -- is as good as
any. Dependencies take release's 16 rather than dev's 256, since they are compiled once and run
for weeks; 1 would be a little better and roughly double that one compile, and is not taken until
a hot path shows it earns it.

**Hot code of our own goes in a crate of its own.** Profile overrides are per package, not per
module, so the way to have a compute-heavy part optimised while the window stays quick to rebuild
is to make it a package: `[profile.dev.package.<name>] opt-level = 3`, and it recompiles only when
it changes. Until such a part exists there is nothing to split; when it does, that is the shape,
not a lower unit count on the whole.

**Release asks what the application costs at rest.** A download manager sits in the background
for hours, so the binary's size on disk and in memory matter more than how long it took to build:
`opt-level = "z"`, fat LTO, one codegen unit, symbols stripped. `panic = "abort"` is a safety
property as much as a size one -- a panic unwinding through a GPUI callback is not a state worth
continuing from, and aborting is what releases the window cleanly.
