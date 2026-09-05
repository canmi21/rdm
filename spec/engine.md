# The transfer engine

## A module of its own, with a runtime of its own

The engine is `src/engine`, one module of the one package, and nothing outside it reaches past
`engine::`: the window sees `Engine`, `Request`, `Settings`, the events and the snapshots, and
none of the pieces under them. It is a module rather than a crate because nothing else needs
the library yet, and a boundary that is kept -- one entry point, no window in it, its own tests
-- is what makes it a crate the day something does: the directory moves, `crate::engine::`
becomes `crate::`, and that is the whole of the work. It was a crate for a week; the split
bought nothing while there was one user, and cost a second `Cargo.toml` to keep in step. Until
the window is wired to it, the module allows dead code at its root, since every item in it is
unreached from the binary; the allow goes with the wiring.

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

The segments sit in the plan in the order they were made, not the order they lie in the file:
a stolen half is made after the segments on either side of it. Anything that judges a plan --
the check that it covers its span exactly once before a plan from disk is believed -- sorts
by position first. The first version of that check did not, and refused every plan a
multi-connection download had written.

The planner is pure arithmetic in `engine/segments.rs`, tested without a network. It is also what is
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

## The probe is a GET for one byte

The first request asks for `Range: bytes=0-0`. A 206 answers three questions at once -- the
size from `Content-Range`, that the server honours ranges, and the ETag or Last-Modified that
later requests carry as `If-Range` so a changed file comes back as 200 and a fresh start rather
than as a slice of something else. A 200 says the server ignored the range: the whole file is
on its way and is dropped unread, the size is `Content-Length` if there is one, and
`Accept-Ranges: bytes` is remembered but not believed, since some servers promise it and do not
keep it. A HEAD would cost the same and answer less: servers that ignore Range on HEAD and
honour it on GET are common, and the reverse is not.

The file's name comes from `Content-Disposition` when the server gives one, the starred form
first because it is the one that can spell a name outside ASCII, else the address's last path
segment, decoded, else `download`. Whatever the source, it passes through one function that
strips separators, control characters and a leading dot, because a name is about to become a
path and a server does not get to choose where on the disk it lands.

## Tests run against a server of their own

`engine/testing.rs` is an HTTP/1.1 server on std threads, in the test build only, serving one body
with whatever misbehaviour a test asks for: no ranges, ranges advertised and ignored, a wrong
status, a redirect, a chunked body with no length, a connection dropped part way through, bytes
doled out slowly. It logs every request so a test can say what the engine did -- which ranges
it asked for, how many connections it held open at once -- and that log is how the segment
algorithm is tested without a network. It runs on its own threads rather than the runtime under
test so that a hang in one cannot hide in the other. A handful of tests against a public mirror
exist for the sake of a real network and real sizes; they are ignored by default and run on
request.

## One file, written at offsets

Every connection writes into the same partial file, `name.downloading`, at its own offset with a
positioned write -- `pwrite` underneath on Unix; on Windows `seek_write`, which moves the
file's cursor and may stop short, so the writer loops and nothing reads that cursor -- so
there is no shared cursor between connections, no lock between them and nothing to merge at
the end: the last byte lands and the file is renamed. The
file is grown to its full length before the first byte when the size is known and
preallocation is on, so a full disk fails the download at the start and not at the end, and so
every segment has somewhere to land from the first moment. A partial file from an earlier run
is opened and kept; it is only ever grown. When the final name is taken, the new file becomes
`name (1).ext`, as browsers do, and the caller is told where it went.

The plan is written beside it as `name.rdm`, the same shape and the same rules as state.json:
whole, to a sibling and renamed over, with an integer version that moves only when an older
file could not be read. A control file this build cannot read is an error and not a fresh
start, so a partial file somebody meant to keep is not begun again over.

## The limit is on the sum

Pacing is a token bucket per download and one for the whole engine, both shared by every
connection of every download they cover, so a limit is a limit on the total and not on each
connection. A bucket holds at most a second's worth, so lowering a limit does not pay out a
burst saved under the old one; setting one where there was none starts with a second's worth,
so the first draw after does not wait. A draw larger than the bucket goes through once the
bucket is full and leaves it in debt, which the draws after pay off -- a single large chunk
must not wait forever.

## A connection's end is read from the plan, not from the request

A connection asks for `bytes=start-`, open at the far end, even when its segment has one. The
segment's end can move closer while the connection runs -- that is how a free connection takes
the far half -- so the end is read from the shared plan at every chunk and the bytes past it
are simply not written; the connection then drops the stream, which closes it. Asking for an
open range costs nothing and saves a request when a cut is undone. The end can even move to
inside a chunk already written, since the write and the cut are not one step; the bytes are
right where they are and the far half will write the same ones, so the segment is marked
complete at its new end and nothing is undone.

A request that does not start at the file's first byte carries the validator as `If-Range`, so
a changed file is answered with 200 and the whole file, which the connection refuses as a
change rather than splicing into what is on disk. A 200 to the one request that does start at
the first byte is a server ignoring ranges, and harmless for that segment alone.

## Connections grow one at a time, and each failure is retried on its own

In automatic mode a download starts with `min` connections and is allowed one more each time a
connection delivers its first byte, up to `max`: a server that accepts the first is asked for a
second, and one that is slow to answer is not flooded. A new connection takes an idle segment if
there is one and otherwise cuts the largest remainder, as the planner describes, and it is
started the moment growth is allowed rather than at the next tick, because a small file is over
before a tick. Without automatic mode the span is cut into `max` pieces at the start.

A connection that fails is retried on its own, from where its segment stands, after a wait
that doubles from `retry_wait` each time and up to `retries` times; the others keep running.
Only a failure that trying again cannot fix -- a refusal, a changed file, a full disk -- or
one that has used up its tries stops the download, and then every connection is cancelled and
the plan is written so the download can be picked up later. Cancelling is the same path: the
plan stays beside the partial file, and a cancelled download is a paused one until somebody
discards its files. The plan is also written every half second while connections run, and at
every segment's end, so a crash loses at most a moment.

## The engine is a queue, and the window talks to it in three ways

`Engine` is what the application holds: it starts the runtime, keeps the downloads, runs at
most `max_active` of them at once and starts the next as one ends, and carries the limit on
their sum. The window talks to it three ways and no other. **Commands** -- add, pause, resume,
remove, the limits -- are plain calls that return at once. **Events** -- started, progress at
an interval, completed, failed, paused, removed -- arrive on a standard channel the window
reads at its own pace; the sender never blocks, so a slow window costs the engine nothing.
**Snapshots** answer for any download's state on request, for the frame that needs a number now
rather than the last one sent. Nothing crosses the boundary as a future, so the window's
executor is never the engine's concern.

Pause cancels the connections and keeps the plan; resume queues the download again and a new
run continues from the plan. Remove forgets the download and, when asked, discards the partial
file and plan -- once the download has actually stopped, since it writes its plan on the way
out and a plan written after the discard would be a ghost. A completed file is never deleted
by the engine; it is the user's.

## After the last byte

A checksum the caller supplies -- SHA-256, SHA-512 or MD5, written any of the ways people write
them -- is checked against the finished file, and a file that fails is deleted, because a file
that is not what it should be is worth nothing and a retry must not find it and stop. The
file's kind is read from its first bytes with `infer`, which knows a PNG from a ZIP better than
the extension the server chose, and is reported in the snapshot for the window to draw.

## Three tests reach the network

`engine/mirror.rs`, in the test build only, downloads public files that have been served with ranges for years -- 20 MB
over plain HTTP from thinkbroadband's test files, a few megabytes over HTTPS from kernel.org's
mirror -- with several connections, compares a split download with a single-connection one
byte for byte, and checks a range against the slice of the whole. They are ignored by default
and run with `--ignored`; each is bounded by a timeout so a mirror that is down fails the test
rather than hanging it. Two hosts were tried and dropped before these: one had gone away, the
other stopped answering after two 100 MB pulls, which is the nature of public mirrors and why
these tests are not in the default run.

## Mirrors are checked by size, the origin by its validator

A request may name other addresses of the same file. Connections are spread across the
sources by segment, and a connection that fails moves to the next source with each retry, so
a mirror that dies mid-file costs a retry and not the download. Only the first address is
probed, and only it is trusted with `If-Range`: a mirror carries its own ETag, and asking it
about the origin's would make every mirror look like a changed file. A mirror is held to the
size instead -- the total in its `Content-Range` must be the one the probe saw -- which is what
aria2 does, and enough to refuse a mirror serving a different file before a byte of it lands.

## The window reads a channel on a timer

The window holds the engine and the receiving end of its events. A task on gpui's executor
wakes every 200 ms, drains whatever arrived with `try_recv`, applies each event to the row it
names and asks for a redraw only if something did; the engine's tokio threads never touch a
gpui entity, and the window never awaits a tokio future. A command from the window -- pause,
resume, remove -- changes the row at once and lets the engine confirm by event, so a click
does not wait on a connection closing. The row's id is the engine's task id, so nothing maps
between them. The download folder is the platform's own as the user has it, from the
`directories` crate: the XDG user-dirs entry on Linux, the known folder on Windows,
`~/Downloads` on macOS, which offers no way to move it.

## An address is looked at before it is downloaded

`inspect` runs the probe and, when the server calls the address a web page -- `text/html` or
XHTML in `Content-Type` -- reads the page for the files it links to: every `href` and `src`
value, resolved against the page, kept when it is an http address whose last segment has a
short alphanumeric extension that is not itself a page's, deduplicated, in the order written.
The page is scanned for attribute values rather than parsed as a document, because a page of
downloads offers its files as plain links and a page of anything else mostly does not; what
this misses -- links built by script -- no parser would find either. At most two megabytes of
the page are read. The answer, or the failure's message, arrives on a channel the window polls
like the events, so the check never holds the window. The window uses it to say "this is a
page" before saving one, and to offer the files behind it instead; see [ui.md](ui.md).
