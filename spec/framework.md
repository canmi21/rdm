# Framework

rdm is a macOS application drawn with [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui),
Zed's UI framework. The workspace's default stack for an application is Rust, and GPUI is the
one Rust toolkit whose rendering, text and window handling are exercised daily by a shipping
macOS editor; the alternatives were judged on that and not on API taste.

## Where the crates come from

`gpui` and `gpui_platform` are git dependencies on Zed's repository at a stable release tag, the
same tag on both. Zed's own crates.io release of `gpui` stopped at 0.2.2 in October 2025, while the
framework kept moving with the editor, and inside Zed it is a workspace member like any other: its
manifest inherits versions from Zed's root and reaches its siblings by path. A git dependency is
the one way to take it as Zed builds it -- cargo checks the repository out, finds the crate by name
and reads the root manifest the crate expects -- so the source never names a supplier and there is
nothing to rename.

**The crates.io mirror `gpui-unofficial` was the first choice, and it was dropped** because what it
added was the failure. It republishes each crate on every Zed release tag, and to do that it
rewrites manifests and build scripts; `gpui-apple`'s build script came out looking for a sibling
directory that exists in Zed's workspace and not in the registry, so it panicked on install. 1.19.2
and 1.20.2 both carried it, the mirror's fix for it referred to a constant nothing defined, and the
1.21.0 release failed to publish on that fix -- three weeks held at 1.18.1 while Zed shipped every
week. The mirror's own CI builds from its workspace, where the path resolves, so it could not see
the fault its users hit.

Two other alternatives, still rejected:

- **Zed's `main`** pinned by commit tracks the framework to the hour, at a hash nobody can read.
  A release tag is the version the editor shipped and says which one it is.
- **`gpui-ce`**, a community fork, is not a mirror: it adds and diverges, and its crates.io
  history at the time was three versions, two of them yanked, on a number that collides with the
  official crate's.

**What the tag costs is that cargo never moves it**, so `.mise/tasks/update` does what the mirror's
`version = "1"` did: it asks the remote for Zed's stable tags, rewrites every Zed tag in
`Cargo.toml` to the newest of the same major, and names a newer major without taking it -- a
major here means a Zed 2.0. The workspace's `mise run update` runs it before `cargo update`, so a
release that breaks the build is reverted like any other; see
[spec/toolchain.md](../../../spec/toolchain.md), "Dependency policy". A stable tag is exactly
`vX.Y.Z`; the `-pre` tags are prereleases and are never taken.

The other costs: the first build clones Zed, some half a gigabyte, into `~/.cargo/git`, which CI's
cache keeps. And Zed's `[patch.crates-io]` does not reach a dependent -- cargo applies only the root
workspace's -- so gpui builds here against the crates.io releases of the few crates Zed patches,
async-task and calloop among them, as it did from the mirror.

**The platform backends are a second dependency.** Zed split them out of `gpui` into
`gpui_platform`, so the entry point is `gpui_platform::application()` and `Application::new()`
no longer exists.

## The tray speaks StatusNotifierItem, so gtk3 is not in the tree

`tray-icon` is the tray on macOS and Windows and is not a dependency on Linux, where the tray
is `ksni` instead: the application puts a StatusNotifierItem on the session bus itself. This is
not a preference. `tray-icon`'s only Linux backend is `libappindicator`, which is gtk3, and it
reaches gtk3 a second way through `muda`, which is not an optional dependency of it -- so
turning a feature off does not help and the crate has to leave the Linux graph entirely.

**What gtk3 brought with it was an advisory with no exit.** `gtk 0.18.2` requires `glib ^0.18`,
and `glib` before 0.20 carries RUSTSEC's unsoundness in `VariantStrIter::impl_get` -- a `&p`
passed where a C function writes through the pointer, which recent compilers optimise away into
a null dereference. No version bump reached the fix: `gtk 0.18.2` is the last of the gtk3
bindings, gtk-rs having ended that line, so `cargo update -p glib --precise 0.20.0` is refused
by the resolver naming `gtk` as the reason. The choice was to carry the advisory or to stop
speaking gtk, and StatusNotifierItem is what the desktops read now in any case --
libappindicator is a deprecated shim in front of it.

`ksni` brings `zbus` and no C binding at all, so the Linux build links against nothing for the
tray and `libgtk-3-dev` is gone from the workflow. `Cargo.lock` holds no `glib`, `gtk`, `atk`,
`gdk` or `libappindicator` for any target. **press is a separate matter**: it reaches the same
gtk3 through Tauri, which is gtk3 on Linux by design, and nothing here fixes that.

Two things about ksni that are not obvious. It insists on an async runtime feature even for
the blocking face -- `blocking` alone does not build -- so the dependency keeps the default
`tokio` and adds `blocking` to it, which suits an application whose own loop is gpui's and not
tokio's. And a StatusNotifierItem carries ARGB32 in network byte order while the PNG decoder
hands back RGBA, a difference that is invisible on any machine that does not run a Linux
desktop; `src/tray.rs` converts, and tests the conversion everywhere.

## Text needs a feature flag

`gpui_platform` is depended on with `features = ["font-kit"]`. Without it the macOS backend
draws no glyphs at all and says so once at warn level through `log` -- and with nothing
listening, the first build of this application was a window of colored bars and no words.
`env_logger` is installed at warn level in `main` so the next message of that kind reaches the
terminal instead of nowhere.

## Reading the framework

The crate source is on disk under `~/.cargo/registry` once fetched, and Zed's `crates/gpui/examples`
are the best documentation there is. Two facts learned from it that are not written anywhere
else:

- `Svg` paints only when the color is set on the svg element itself; it does not inherit the
  surrounding text color. See [icons.md](icons.md).
- gpui_macos sizes the traffic-light strip to `button height + 2 * y` and hangs it from the top
  of the window, so the `y` in `TitlebarOptions::traffic_light_position` is the padding on both
  sides. See [ui.md](ui.md).

## The text field is Zed's example, kept

GPUI ships no text input; Zed's editor is its own crate and far more than a field. The one-line
field in `src/ui/text_input.rs` is the framework's `examples/input.rs` (Apache-2.0), trimmed to one
line, drawn in this palette, and given a confirm callback for Enter. It implements
`EntityInputHandler` so the system's input method, dead keys and the character palette work, which
a hand-rolled key handler would not get right. Its key bindings are bound once in `main`, scoped
to the `TextInput` key context.

The example carried a bug that only an input method reaches: the selection it computed after
re-marking a composition added the replaced range's _end_ to the new selection's end, which put
the selection past the content, and the next replacement sliced out of bounds. Latin typing never
marks text, so the field looked fine until Chinese was typed into it. The arithmetic is corrected
and every range that slices the content is clamped to it and to character boundaries; the
headless tests drive the field the way an input method does, keystroke by keystroke.

**Enter and Escape reach the field's owner after the field is done with itself.** The field runs
its key actions inside its own update, and an owner acting on Enter reads its fields -- the New
Task sheet reads the address Enter was pressed in -- which GPUI refuses with a panic for an entity
already being updated. Pasting an address and pressing Enter took the application down that way.
So the callbacks go through `Window::defer`, which runs them once the update has finished;
`Context::defer_in` is not enough, since it runs its callback inside another update of the same
entity. A headless test presses both keys and reads the field from the callbacks.
