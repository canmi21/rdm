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
segment finished -- it takes an idle segment if there is one; otherwise it cuts the segment that
will finish last where its remainder halves, and takes the far half. Which will finish last is
its bytes left at its own pace, each segment's pace measured every half second; a segment with no
pace yet is taken at the typical one. The segment with the most bytes left was cut at first, and
with connections at different speeds that is not the one holding the download up: a slow
connection with less left went on alone after the rest had finished. The near half never
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
honour it on GET are common, and the reverse is not. The one byte of a 206 is read to its end,
which puts the connection back in the client's pool, and that client is the download's first
connection -- below, "The server decides how many connections it takes".

The same answer says something of the server too: the protocol version it came over and its
`Server` header. The probe keeps both for Add Task to show beside the size and the date, and
nothing after it reads them -- they decide nothing about how the file is fetched, so a server that
sends no `Server` header is simply not named.

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
it asked for, how many connections it held open at once, how many sockets it accepted for them --
and that log is how the segment algorithm is tested without a network. It keeps a connection alive
between requests, as a real server does, so a client's pooled connection is asked again on the
same socket. Told to limit connections, it judges each by the count it arrived to, as a server
admitting them in turn does: read when its request came, the count took in connections that
arrived after it, and two arriving together were each turned away for the other. It runs on its own threads rather than the runtime under
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

## Connections grow by rounds, and each failure is retried on its own

Every setting here reaches the window: the ones that hold for every download are the
Transfers section of Settings, kept in `config.json` and written over the engine's defaults
for each new request, and the ones a download can have of its own -- the folder, the name, the
checksum, the range and a limit -- are New Task's, while the connections and the
limit are its window's, changed as it runs; all of them are kept on the row. A download asks for connections in one of two shapes, `Connections::auto` or
`Connections::fixed(n)`, and never more than `Connections::MAX`, 32, whatever it asks -- automatic
mode's own ceiling, and past it nothing is gained: on a local network one connection fills the
link, a public mirror that is slow on one fills a gigabit on sixteen to thirty-two, and a fixed
sixty-four on a real mirror finished later than automatic thirty-two, its pieces cut too small at
the start to be cut again at the end; it was 256, which only asks a server to ban the address. In
automatic mode a download starts with `min` connections, four, and is allowed two more each time
a connection delivers its first byte, up to `max`, thirty-two: each round of answers doubles the
count, as TCP's slow start does, so a server that takes many is reached in a few round trips, and
one that is slow to answer is not flooded, since nothing grows until something answers. A server
that takes fewer turns the excess away and the count comes down to its limit, below; being
aggressive first and backing off after is what reaches the speed another downloader gets from the
same server, where adding one at a time stopped short of it. A new connection takes an idle segment if
there is one and otherwise cuts the largest remainder, as the planner describes, and it is
started the moment growth is allowed rather than at the next tick, because a small file is over
before a tick. Without automatic mode the span is cut into `max` pieces at the start.

A connection asks for its segment and no further, `bytes=start-end`, so the server finishes the
answer where the segment ends and the connection closes cleanly; a steal that cuts the segment
while it runs has the writer stop at the new end and the connection dropped early. It asked
open to the file's end at first, reading past its segment being cheaper than a new request, and
every finished segment then left the server writing into a connection we had dropped, counted
against its limit until it noticed.

A connection that fails is retried on its own, from where its segment stands, after a wait
that doubles from `retry_wait` each time and up to `retries` times; the others keep running. A
connection the server turned away is another matter, below.
Only a failure that trying again cannot fix -- a refusal, a changed file, a full disk -- or
one that has used up its tries stops the download, and then every connection is cancelled and
the plan is written so the download can be picked up later. Cancelling is the same path: the
plan stays beside the partial file, and a cancelled download is a paused one until somebody
discards its files. The plan is also written every half second while connections run, and at
every segment's end, so a crash loses at most a moment.

## A stuck connection is reopened

A connection can stop without failing: the server stops sending, or a path goes bad and a
connection that kept pace with the others drips a few kilobytes a second. The transport's own
`idle_timeout`, a minute, catches only the first, and only after the minute; the second it never
catches, and the download sits at ninety-nine percent on its last connection until somebody pauses
it and starts it again -- which is what that does, and what the engine now does itself. Each tick
the scheduler looks at every connection that has run for `stall_timeout`, ten seconds:

- **Stopped.** Nothing landed for `stall_timeout`, from a connection that has answered or is the
  only one left.
- **Crawling.** For `stall_timeout` on end, under a sixteenth of the others' pace -- the median of
  the rest, or for the last one left, the fastest the others kept while there were two -- with
  more left than that pace would finish in `stall_timeout`.

Either, and that connection alone is dropped: its segment goes back, and a new connection picks it
up from where it stands. One dropped without a byte since it opened spends a try, so a server that
never sends anything still fails the download when the tries are gone. Nothing is judged on a
server without ranges, which cannot be asked to go on from the middle, or under a speed limit,
where a slow connection is the limit working. Each connection has its own stop beneath the
download's, so the scheduler tells its own drop from a pause by whether the download's was pulled.

## The server decides how many connections it takes

Many servers cap how many connections one client may hold, and say so badly: a 429 or a 503,
sometimes with a `Retry-After`; a 403 to the connection past the limit while the others go on
being served; or a connection closed before it has answered at all. The engine used to take each
of these as the download failing -- a 403 at once, the others after their retries -- and a server
that took two connections refused a download that asked for eight. **So the count is learnt as it
goes, the way TCP learns a window: additive increase, a drop on a refusal.**

- **Up by rounds.** Two more are allowed each time one delivers its first byte, as above.
- **Down to what the server is serving.** A connection turned away while others run is the limit
  found, and the count is set to how many are running: the one number the server has just shown
  it takes, which is closer than halving and never further. The connection's segment goes back
  for whoever comes free, no try is spent on it, and no new connection opens until the wait the
  server asked for -- or `retry_wait` -- is over.
- **Not for a connection of ours still closing.** A server goes on counting a connection we have
  closed until it notices. A refusal within half a second of one of ours that was being served
  closing, or at a count the server has already served, or of a connection opened within half a
  second after one of ours was turned away, is waited out without lowering anything; three of
  those in a row, with nothing answering between, and the limit is taken as lowered. Only one
  opened after the refusal: the rest of a round, asked for at the same moment, were turned away by
  the same full server and count, and doubting them too learnt limits of three from a server that
  takes two. Without this a download learnt a limit of one from a server that takes two, a little
  more than half the time.
- **One, and then failure.** Turned away with nothing else running is the limit at one, and the
  connection is retried as any other, after the longer of its doubling wait and the server's.
  Only when a single connection's tries are used up does the download fail.
- **Back up while it holds.** Ten seconds at the limit, every connection in use and none turned
  away, and one more is asked for, so a limit learnt at a bad moment does not hold for good.
- **Remembered by host.** What a run learnt is kept per host and handed to the next download from
  it, which starts at that limit instead of finding it again; a run that was never turned away
  keeps one more than it reached, so the next asks a little further; and a host that reaches the
  most a download may ask for is forgotten. The window keeps the table in `state.json`.
- **The probe's connection is the first one.** Its client, with the connection in its pool, is
  handed to the download's first connection, which asks for its segment on the same socket. Kept
  idle beside the download, it held one of the places the server counts for the whole of it;
  dropped, the server went on counting it until it noticed the close, and the first round -- sent
  at that moment -- was turned away one short and learnt a limit of one from a server that takes
  two.

The failures are told apart in `engine/task.rs`, which the tests drive against a server that turns
away connections past a limit with a 503, a 403, or a close, and which counts what it turned away.
The same download against the busy server made two dozen requests before this, most of them
refused. It is AIMD rather than a borrowed library: the concurrency-limit families -- Netflix's
`concurrency-limits` among them, with Vegas and gradient schemes -- judge a limit from latency,
and a download's latency says little about how many connections a server will take.

## The engine is a queue, and the window talks to it in three ways

`Engine` is what the application holds: it starts the runtime, keeps the downloads, runs at
most `max_active` of them at once and starts the next as one ends, and carries the limit on
their sum. The window talks to it three ways and no other. **Commands** -- add, pause, resume,
forget, discard, the limits -- are plain calls that return at once. Forgetting a download leaves
its files; discarding it takes the partial file and the plan, found by the name it was written
under -- the caller's, else the server's the probe learnt -- and never a finished file. It was one
`remove(id, delete)`, a flag nobody could read at the call, which found nothing to delete when the
server had chosen the name. **Events** -- started, progress at
an interval, completed, failed, paused, queued again, removed -- arrive on a standard channel the window
reads at its own pace; the sender never blocks, so a slow window costs the engine nothing.
**Snapshots** answer for any download's state on request, for the frame that needs a number now
rather than the last one sent. Nothing crosses the boundary as a future, so the window's
executor is never the engine's concern.

## The queue can be reordered

At most `max_active` downloads run and the rest wait, in the order of a ticket each holds: a new or
resumed download takes one at the back. Two calls move a download out of its turn, and both go by
stopping a run the way a pause does -- the plan kept, so nothing is fetched twice -- except that
the stopped download is sent back to the queue, with `Event::Queued`, rather than paused.

- **`yield_place`** sends a running download to the back, so the next waiting one starts. With
  nothing else waiting it does nothing, since it would only be started again.
- **`start_now`** gives a waiting, paused or failed download a ticket ahead of everything waiting.
  With every place taken, one running download gives way and waits at the front, next after it, so
  it goes on as soon as a place frees. Which one is `EngineSettings::bump`: **the one that started
  last**, the default, which has had least time to reach its speed and so loses least by stopping;
  the one with the most time left at its pace; or the slowest. The user chooses in Settings.

A pause of a download that is giving its place away wins: stopped, it stays stopped. `restart`
begins a download again from nothing -- the partial file and plan deleted, then a ticket at the
back -- and refuses while it runs, which would race its own files; a finished file is the caller's
to move out of the way, which the window does by sending it to the Trash.

Pause cancels the connections and keeps the plan; resume queues the download again and a new
run continues from the plan. Remove forgets the download and, when asked, discards the partial
file and plan -- once the download has actually stopped, since it writes its plan on the way
out and a plan written after the discard would be a ghost. A completed file is never deleted
by the engine; it is the user's.

**A failed look comes back as a sentence and the whole story.** `Error::summary` says what went
wrong in terms a person can act on -- a status with its reason, a server that could not be reached,
a wait that ran out -- and `Error::detail` is the error's text with every cause under it, each said
once, which is what the transport actually said. `Engine::inspect` sends both; New Task shows the
first and keeps the second behind Details, and neither is parsed back. `inspect::confirmation` names
the answers New Task asks about before going on: a page, a script, a stylesheet.

## After the last byte

**A checksum is checked only against a whole file.** A download of a part of one has nothing it
could be compared with, so New Task does not take both and the application hands the engine no
checksum for a row with a range. The engine itself would check whatever it was given; the rule
sits where the row becomes a request, in `src/app/transfers.rs`.

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

## TLS is rustls, and the crypto under it is this crate's choice

There is no OpenSSL in the tree: no `openssl-sys`, no `native-tls`, nothing linking `libssl`.
Every connection that is encrypted -- a download, the update check, DNS over HTTPS -- goes through
**rustls**. The one crate whose name looks like an exception is `openssl-probe`, which reads the
paths a system keeps its CA bundle at and links nothing.

Under rustls sits a provider, and **which one is decided here rather than by whichever dependency
happened to ask first.** Left alone, reqwest's `default-tls` brings AWS-LC and the resolver's
`https-ring` brings ring: two crypto libraries in one binary, doing one job, with the winner
decided by installation order. So reqwest takes `rustls-no-provider` and the resolver takes no
provider feature of its own, and the crate's own `aws-lc-rs` and `ring` features answer the
question in one place.

**AWS-LC is the default**, and it is what the release builds carry. It is a C library that cmake
builds, which is a build dependency the release machines already have -- the nightly has been
building it on macOS, Linux and Windows since before it was written down here, because reqwest's
default brought it. Its lineage runs back through BoringSSL to OpenSSL, so the C is related, but
it is not the OpenSSL library and none of it is linked as one.

**ring is the other build**, with `--no-default-features --features ring`: no cmake, no C
toolchain, for a machine or a target that has none to spend. Both features are additive, as
Cargo's are, so AWS-LC wins when both are on and a `--features ring` that forgot
`--no-default-features` builds the default rather than failing. Neither is a compile error, said
in `src/tls.rs` rather than discovered as a missing symbol.

The provider is installed once, by `tls::install`, and **every client asks for it before it is
built** -- the engine's, the update check's twice over, and the resolver's. Not in `main`, because
a test never runs `main` and would otherwise reach a connection with nothing to encrypt it; the
two ignored network tests are what prove the arrangement, and they are run against both features.

## The proxy is looked for before it is asked for, and it is given the name

The machines this runs on usually have a proxy on them, and the address it listens on is the
program's choice -- mihomo picked 7890, not the user -- so asking somebody to type it is asking
them to know something about their own machine that the machine can be asked instead. `Whatever
is running` is the default: a socket is opened to each known address in turn and the first that
answers is the one every download goes through; nothing answering is the same as no proxy at all.

**Three things are looked for, not a catalogue of programs.** `src/proxy.rs` holds the list: the
mixed port on its old and new defaults, the SOCKS port that usually sits beside it, and the
address a SOCKS5 proxy has had since before any of them. What matters is not which program is
listening but what it speaks, and a list of every proxy's default port is a list to fall behind
on. Ports that something other than a proxy commonly holds are left out on purpose -- 8080 is
somebody's development server far more often than it is a proxy, and sending every download
through one would be worse than finding nothing. Only loopback addresses are probed, and a
connection opening is all that is asked: speaking the protocol to find out whether it is really a
proxy would mean sending a request through a program the user has not agreed to send anything
through.

The look runs off the window's thread, at launch and whenever it is asked for. What is found is
shown in Settings, can be overruled by an address typed there or turned off entirely, and is not
written to `config.json`: it is a fact about the machine now rather than a choice somebody made.

**A proxy is given the name, never an address.** The address a CDN answers with depends on who
asked, and the one that matters is the one seen from where the connection is actually made -- an
address resolved here and handed to an exit in another country sends it to a node picked for this
one. A proxy that routes by rule is handed the domain its rules are written against, and one
handed an address can only fall back to matching on the address. And a local network that answers
with a lie has its lie carried into a channel that would have got the truth. All three point the
same way, so nothing is resolved for a request that goes through a proxy: not our resolver and
not the system's either, the name travelling in the CONNECT line or in the SOCKS request.

`socks5://` and `socks5h://` are the same wire protocol and differ only in who resolves, which is
curl trivia rather than something a settings field should make somebody know: whatever is typed
or found, the client is given the form that sends the name.

## Names are resolved here, the same way on every platform

**This application resolves names itself.** Not for safety -- asking the machine's own servers
with our own client gets the machine's own answers, lies included, and what buys trust is
changing who is asked or how. The reason is that one Rust stack, one cache and one set of
timeouts behave the same on macOS, Windows and Linux, so a download that will not start is one
thing to reason about instead of three platform resolvers with three sets of habits. It is
`hickory-resolver`, reading the machine's own servers to begin with: on Apple from the
SystemConfiguration store rather than `/etc/resolv.conf`, so the search domains arrive with the
addresses.

reqwest's own `hickory-dns` feature is deliberately off. It builds from the system's servers and
offers no way to name others, falls back to Google's when it cannot read them -- a change of who
is asked, announced in a debug log -- and would leave two places in the tree that build a
resolver. The version is held to reqwest's anyway, so turning it on stays a one-line change
rather than a second copy of the crate.

**Port 53 is asked over UDP, and the same question moves to TCP by itself** when an answer comes
back truncated, and also when it comes back with the wrong case, which is how hickory catches a
forged reply. A question that simply times out is retried as it was; making it a TCP question
would be a branch in the default path that only the network being hostile could justify, and a
hostile network is what DNS over HTTPS is for.

### The chain, and why the fallback is an escape hatch rather than a second opinion

What the system's stack knows and no unicast server does is `.local`, answered by multicast, and
whatever a VPN's own scoped resolver answers for -- neither of which is in the machine's global
DNS configuration, so reading that configuration does not bring them along. A name our resolver
cannot find is therefore put to the system before the download fails.

**Under DNS over HTTPS there is a rung between them:** our own stack on port 53, on the machine's
own servers. HTTPS falls to it and it falls to the system. **A request carried by a proxy skips
it** -- one that has got this far wants the machine's stack, not a second question of ours asked
from a place the connection is not made from. **Forcing HTTPS removes the chain entirely**, since
both rungs below would send the question out in the clear and that is the one thing forcing it is
for.

Whatever the chain, it runs where nobody could have answered and not where an answer came back
that somebody might not like. A question that never got through walks it whatever the servers
were, each rung being a different path to a different set of servers. A name that does not exist
walks it only while the servers being asked are the machine's own: somebody who named servers
said this machine's are not to be trusted, and a fallback that asked them anyway would hand back
exactly what was refused.

### The names the machine answers for, which never reach the chain at all

Because of that rule, a name only this machine can resolve stops resolving the moment servers are
named -- so those names are declared rather than discovered. A short list goes to the system's
stack before anything else is asked, and never through a proxy either, a name on the local network
being one a proxy can neither resolve nor reach. It is a routing rule and not a fallback: it holds
whatever else is set, forced HTTPS included, since no DoH server has ever been able to answer for
`nas.local` and asking one is not stricter, only broken.

**Exactly one entry is built in, and it is `.local`.** Where that one goes is not a policy
question: it is answered by multicast and no unicast server has it, so there is no other right
answer and a resolver that sends it to Cloudflare has broken `nas.local` for nothing. **Every
other internal domain is somebody's arrangement and not ours to guess.** `.lan`, `.home`,
`.internal`, `.corp` and a company's own name are all real, all different, and all in use for
public names somewhere; a built-in list of them would quietly take names away from the servers the
user chose, which is the one thing choosing servers is meant to prevent. So the rest is a field,
and a domain in it stands for itself and everything under it, as `NO_PROXY` has always meant.

### One resolver, for the life of the process

A resolver holds a cache, and a cache thrown away with the client that made it answers nothing
twice -- a download builds a client per connection, so this is the difference between one query
for a name and sixteen, and between one DoH handshake and sixteen. So one is built and kept, with
the choice that built it beside it: a settings change replaces it, and it is never left answering
with servers the settings used to name. Whether a proxy carries the requests is kept beside the
choice too, not being a setting but deciding the chain, so the two shapes never share one.

It is built at the first name asked rather than where it is made, because building it may have to
look a DoH server's own address up, which is a question, and a question wants a runtime.

### Four switches, and each turns something off

- **Force the system's resolver.** Off. On, nothing of ours is built and reqwest resolves the way
  everything else on the machine does. It is the way out if resolving here is ever the problem,
  which is worth having on the screen rather than in a config file.
- **DNS over HTTPS.** Off. On, the question cannot be read or rewritten on the way, which is what
  somebody whose network answers `github.com` with a lie is after. It buys integrity and not
  reach: a DoH server that is blocked is blocked. It also outranks the proxy rule above -- a
  request through a proxy resolves here after all, because who may see and answer a question is a
  stronger claim than where its answer should come from. Turning it on beside a running proxy
  points two things at one job, which is the user's to settle and the reason the switch is off
  until they turn it on.
- **Force DNS over HTTPS.** Off, and only on screen beside the switch above, since forcing a
  transport that is not in use says nothing. On, there is no chain: a name that cannot be resolved
  over HTTPS is a download that does not start.
- **Which servers.** The machine's own, one of the two anybody in that position already knows, or
  whatever is written in the field. On port 53 the offered two are named by their address, because
  that is what a person remembers; over HTTPS they are named by their operator, because nobody
  remembers a DoH URL. Choosing one fills the field beside it, as choosing a user agent does, so
  what is being asked is on screen rather than implied.

**Both offered DoH servers have their addresses written down**, so the ordinary way to turn DoH on
never has to ask the network where its DoH server is. A URL may name an address itself; anything
else is looked up once through whatever is already working, which is not a circle -- one question
before the first, and every question after it over HTTPS -- but it does mean a bootstrap that goes
through the thing being worked around. Whatever the address came from, the host beside it is what
the certificate is checked against, so an address rewritten on the way fails the handshake rather
than answering the questions.

An address that will not parse is left out rather than taken as a reason to fail, a settings field
being typed into a character at a time. A field with nothing usable in it is the same as having
named nothing, and the machine's own servers answer -- except over HTTPS, where dropping back to
port 53 would be answering a question nobody asked, and the server offered first is what it comes
to instead.

Four tests in `src/dns.rs` reach the network and are ignored by default, like the engine's own.
The one that matters most goes straight at the built resolver rather than through the fallback:
a DoH server that never answered would otherwise be carried by the system's stack and the test
would pass on an answer the thing under test did not give.

## What this application calls itself, and the two disguises it offers

The honest answer is `rdm/<version>`, and it is what is sent unless somebody says otherwise. Some
servers hand a download manager a different file, a slower one or none at all, so three disguises
exist -- Chrome on Windows, on macOS and on Linux -- and Settings offers **two** of them: never
the one this system would be telling the truth with, since a macOS machine claiming to be a macOS
machine is not a disguise. Choosing one fills the field beside it, so what is being sent is on
screen rather than implied; a disguise nobody can read is one nobody can check. The Chrome major
version lives in one place in `src/agent.rs` and is worth refreshing, a disguise three years out
of date being a disguise and not a good one.
