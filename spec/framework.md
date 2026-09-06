# Framework

rdm is a macOS application drawn with [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui),
Zed's UI framework. The workspace's default stack for an application is Rust, and GPUI is the
one Rust toolkit whose rendering, text and window handling are exercised daily by a shipping
macOS editor; the alternatives were judged on that and not on API taste.

## Where the crates come from

`gpui` is the crates.io package `gpui-unofficial`, renamed back to `gpui` at the dependency so
the source never names the supplier. Zed's own crates.io release of `gpui` stopped at 0.2.2 in
October 2025, while the framework kept moving with the editor; `gpui-unofficial` republishes
`crates/gpui` and its workspace dependencies on every Zed release tag, automatically, with the
version number equal to Zed's. It was chosen over two alternatives:

- **A git dependency on Zed's `main`** tracks the framework to the hour, at the cost of pulling
  the whole Zed repository and pinning to a commit hash that `cargo update` cannot reason about.
  Zed's own release tags are the version that has been run, so following those loses little.
- **`gpui-ce`**, a community fork, is not a mirror: it adds and diverges, and its crates.io
  history at the time was three versions, two of them yanked, on a number that collides with the
  official crate's.

The version constraint is the major, `"1"`, as every pin in the workspace is; `Cargo.lock`
records the exact release. A major here means a Zed 2.0.

**The platform backends are a second dependency.** Zed split them out of `gpui` into
`gpui_platform`, so the entry point is `gpui_platform::application()` and `Application::new()`
no longer exists. The mirror's readme still says one dependency is enough; it is not.

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
re-marking a composition added the replaced range's *end* to the new selection's end, which put
the selection past the content, and the next replacement sliced out of bounds. Latin typing never
marks text, so the field looked fine until Chinese was typed into it. The arithmetic is corrected
and every range that slices the content is clamped to it and to character boundaries; the
headless tests drive the field the way an input method does, keystroke by keystroke.
