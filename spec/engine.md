# The transfer engine

## A crate of its own, with a runtime of its own

The engine is `crates/engine`, the package `rdm-engine`, the library `engine`. It is a crate
rather than a module for three reasons that all point the same way. It is a library: an API
over addresses, files and bytes, with no window in it, and it should be usable by a command-line
front end or a test with nothing of gpui linked. It has its own dependencies -- reqwest, tokio
-- that the window has no use for, and a crate boundary is where Cargo lets them stop. And while
the window is still drawn from mock rows, every function in it would be dead code to the binary;
a crate's public surface is not dead, and the lint stays honest without an allow.

**It owns a tokio runtime.** reqwest is the HTTP client of the Rust ecosystem, and it runs on
tokio; gpui runs on an executor of its own. Zed meets the same fact and answers it the same way
-- a runtime started for HTTP, the rest of the editor never touching it -- so the engine starts a
multi-threaded tokio runtime when it is made and keeps it for its life. The window and the
engine meet only through channels: commands in, events out, snapshots on request. Nothing in the
engine's API is a future the caller has to drive, so the caller's executor is not the engine's
concern. Size was weighed and set aside: correctness first, and the binary can be looked at again
when there is a reason to.

## Segments are the plan, and the plan is what is saved

A download is a span of bytes, `0..size` for a whole file or whatever part was asked for, cut
into segments, each the share of one connection. A segment is written front to back, so the one
number `done` says exactly which of its bytes are on disk; the segments together cover the span
exactly once, and a segment is never removed, only split, so every byte has one owner for the
whole life of the download. That invariant is what makes resuming trivial: write the plan down,
read it back, and every open segment continues from `start + done`.

**Growing the connection count is aria2's "steal", not a fixed cut.** A download starts with
one segment for the whole span. When a connection comes free -- at the start, or because its
segment finished -- it takes an idle segment if there is one; otherwise it cuts the segment with
the most bytes left where that remainder halves, and takes the far half. The near half never
notices: its end moved closer, and it keeps writing towards it. A cut happens only while both
halves would be at least `min_segment` long, so connections stop multiplying where they would
spend more on setup than transfer. This is what "automatic" multi-connection means here; the
non-automatic mode cuts the span into `max` equal pieces at the start, and single-connection is
`max = 1`.

The planner is pure arithmetic in `segments.rs`, tested without a network. It is also what is
serialised beside a partial file so that a download survives the process; the file's shape is
the planner's, and the reasons above are why it can be.

## Settings are the window's future, held ready

Every knob a settings window will show -- connection count and whether it grows on its own,
the smallest segment, timeouts, retries, a speed limit, a size ceiling, the HTTP version, user
agent, headers, proxy, redirects, preallocation -- is a field of `Settings` with the value it has
until somebody changes it. The window is not built and no file is read, so nothing here is
reachable from the screen yet; the point is that when the window is built, it binds to fields
that already exist and already do something.

**Several connections mean HTTP/1.1.** HTTP/2 multiplexes every request onto one TCP
connection, and a download with several connections wants several TCP connections, because
what it is working around is a server's per-connection pacing. So a split download builds its
clients with `http1_only`, one client per connection; the HTTP version setting governs the
single-connection case, where negotiation costs nothing.
