# Rules

What rdm knows about the places files come from, kept as data rather than code: which addresses are
mirrors of one another, and where a file's checksum can be read before it is downloaded. The engine
already spreads one download across several sources (`Request::mirrors`, see
[engine.md](engine.md)); rules are how those sources are found without the user typing them.

## Two kinds of rule

- **A family** is a set of URL prefixes that serve the same tree: swap one prefix for another and
  the same file is there. GNU, Apache, SourceForge, kernel.org and the Linux distributions are
  families. nixpkgs keeps the most complete list of them, `pkgs/build-support/fetchurl/mirrors.nix`,
  under the MIT license, and is where most of ours will come from.
- **An entry** matches one kind of address by a template with named parts -- a GitHub release asset
  is `https://github.com/{owner}/{repo}/releases/download/{tag}/{file}` -- and says, in order,
  where that file's checksum can be read and which other addresses serve it.

Checksums, as found when this was settled: a GitHub release asset carries `digest` in the releases
API for anything uploaded since June 2025 and nothing before, when a `.sha256` beside the asset or a
`checksums.txt` or `SHA256SUMS` in the same release usually answers instead; npm has
`dist.integrity` and `dist.shasum`; PyPI has `digests.sha256` per file; jsDelivr's data API gives
every file's SHA-256; a distribution's ISO directory has its `SHA256SUMS`, and a MirrorCache server
answers `.meta4` with sizes and four hashes.

## Files, groups and layers

Every rule is TOML, one or more to a file, and a file sits in a folder tree whose folders are
groups: `rules/code/github.toml`, `rules/distributions/debian.toml`. TOML because the files are
written by hand and every mirror wants a comment saying why it is there, which JSON cannot hold, and
because YAML's implicit types mistake a bare `NO` for false. The tree lives in this repository, in
`rules/`, beside the code that reads it, so a change to the format and to its reader is one commit.

The application reads three layers and **merges them at start into one compiled set**, held as JSON
-- the form the rest of the program reads -- and rebuilt whenever a layer changes:

1. **Built in.** A few rules compiled into the binary, the ones that are close to fact and are the
   floor when nothing else has arrived: jsDelivr's addresses for npm and GitHub files among them.
   Kept small on purpose; the rest arrives by sync.
2. **Synced.** Every file of the repository's `rules/`, downloaded in the background each time the
   application starts. They live in a `rules` folder in the configuration directory on each of
   macOS, Windows and Linux, and **that folder is the application's**: it adds, replaces and deletes
   files there so it matches what was published, and nothing a user puts there survives a sync.
3. **Custom.** The user's own, in a `custom` folder beside it, which the application never deletes
   from. It writes there only for something the user did -- "Never ask for this source" below.

**A rule's id is its identity across layers**: where two layers hold a rule of the same id, the
higher layer's is the one kept -- custom over synced over built in -- so the synced copy of a
built-in rule replaces it rather than standing beside it, and a user replaces a rule by writing one
of the same id. The built-in layer is the floor until a sync arrives, and was listed twice beside it
until this.

Rules carry a priority, and the merge orders every rule of every layer by it, so a user's rule can
be placed anywhere among the others rather than only before or after all of them. **Where several
rules match one address, the one with the highest priority is used whole** and the rest not at all:
a user changes what an entry does with one rule of their own, rather than by writing exclusions
against every rule whose mirrors would otherwise be merged in.

The synced layer is not signed, for now. What a tampered rule can do is limited by the next section:
a mirror is only used with a checksum from the source, and the list of mirrors trusted to answer
for a source cannot be changed by a sync, so the worst a changed rule does is send a download to a
mirror whose bytes then fail the check.

## The synced layer is fetched whole, from GitHub or jsDelivr

At start and every six hours after -- background work, held back while a download crawls, see
[release.md](release.md), "Background work" -- the repository's `rules/` is fetched into the synced
layer. **GitHub and jsDelivr are asked for the list at once**: GitHub's recursive tree of `main`
and jsDelivr's flat listing of the same branch. GitHub is used when its list arrives within eight
seconds and every file then comes from `raw.githubusercontent.com`; when its list is late or wrong,
or any file fails to come, the whole sync goes to jsDelivr instead, whose listing gives each file's
SHA-256, and a file that does not match it fails the sync. The two are asked together so a slow
GitHub costs its eight seconds and no more, and one source serves a whole sync so its files are of
one moment.

Nothing partial is kept: the files are written to a folder beside the layer, which then takes the
layer's place, so a sync that fails half way leaves the last one standing. A listing with no rules,
more than five hundred, or a path reaching outside `rules/` is refused. The rules are reloaded once
the new layer is in place, and the rules window says when the last sync was, from where, or why it
failed, beside Sync now.

## A mirror is used only with a checksum from the source

A mirror answers with bytes, and nothing about the bytes says they are the file asked for. So a
download takes another address only when it has a checksum to hold the result to, and **that
checksum comes from the source** -- the origin's API, or a sum file on the origin's own host --
never from the mirror being checked.

The one exception is a short, named list of **authoritative mirrors**, which may answer for the
source: jsDelivr is one, since what it serves for a package or a repository is the registry's or the
repository's own content, and its data API gives the checksum of each file.

**That list is written only in the built-in and custom layers, never in the synced one**, so a
sync cannot make a host an authority and so step around the checksum. A mirror the built-in layer
names as authoritative is by that token one in common use, and its rules are built in with it
rather than left to arrive by sync.

## When there is no checksum

The checksum is found automatically wherever a rule says how. Where it cannot be and a mirror rule
matched, New Task asks: after Download on the screen that shows what was found, a further screen
says a mirror was found for this file and offers New Task's checksum field again, with a `?` beside
it explaining that the mirror is tried only when the result can be checked, which is what the
checksum is for. Three ways out below it, the last two remembered for the source's domain -- not
its address -- as a rule written to the custom layer:

- **No thanks** -- this download goes from the source alone, and the next one from here asks again.
- **Use mirror when possible** -- never asked again for this domain; a download from it uses a
  mirror when the checksum can be found by itself, and goes from the source alone when it cannot.
- **Never ask for this source** -- never asked again, and never a mirror for this domain, even when
  a checksum could be found.

## How a rule is written

A template's parts are `{name}`, one path segment; `{name+}`, one or more; and `{name:expression}`,
whatever the regular expression says, braces inside it counted. Every part matches as little as it
can, which is what splits `react-dom-19.1.0.tgz` into a name and a version where the version's first
digit is. The query and fragment are left off before matching -- a signed or tagged address is the
same file -- and `{url}` fills with the rest. A template that fills a part the match does not have
is an error rather than an address.

A checksum source is one of three kinds: `json`, a field of a document reached by a path of keys and
`list[field=value]` filters; `sidecar`, a file holding the sum; `sums`, a list of sums found among a
JSON document's items by name, as a release's `checksums.txt` is among its assets. Every form a
checksum arrives in -- `sha256:hex`, npm's `sha512-base64`, bare hex, base64 with `algo` and
`encoding` beside it -- becomes one checksum, and a source that fails is the next one's turn.

Every entry carries `examples`, real addresses its pattern must match, and a test reads the whole
`rules/` tree and checks each against its own entry and every template it fills; an ignored test
asks the real services for each built-in example's checksum.

## How a mirror is found and trusted

When New Task has looked at an address, the rules are worked out on the engine's runtime while the
second screen is read: the matching entry's checksum and mirror templates, and every family one of
whose prefixes the address starts with. Each candidate is probed and kept only when it serves a
file of the source's size with ranges, since the download is split across them. A checksum found is
written into the checksum field, where it is seen and can be cleared; Download does not wait for an
answer still being worked out, and goes from the source.

A kept mirror is used only when a checksum holds it, and not by one read from the mirror's own site
-- a host's last two labels, close enough to tell a mirror from a source -- unless that host is an
authority. A checksum the user typed holds every mirror: the user is its source. An entry from the
synced layer may read a checksum only from the source's own site or an authority.

A choice made on the third screen is written to `choices.toml` in the custom layer, which nothing
else writes, one `[[domain]]` a domain, `www.` left off; a domain covers the hosts under it. The
merged set is written to `rules.json` beside the state for reading, and rebuilt when a choice is
written.

## A window for the rules

A window of its own, opened from Settings' Transfers under Sources, is **a table drawn as the main
window's detailed list is**: column titles over dense rows of the same height, and a row selected by
a press. The columns are the name, what it matches -- the template, or a family's prefixes -- what it
provides, the layer it came from, and its priority; a row's tooltip holds what the columns cut short.
It was a list of cards grouped under headings, with buttons on every row, and read as a settings
page rather than as the data it is.

**Its foot is one row, the status bar's height**, with a dashed line along its top. At the left, from
the window's edge, the tabs narrow the table to the rules, the mirror families, the authorities or
the choices made in New Task, each with its count, and a tab of what could not be read appears only
while there is something in it. The tabs are bare words; the one showing stands between dashed lines down its sides,
with no space added around it, and every tab keeps a clear border on the same two sides so choosing
another moves nothing. The line along the top is an element under the tabs rather than the row's
border, which gpui paints over its children. Filled pills, a frame on every tab, and the showing tab outlined as a browser's were
each tried and each busier; the tabs had a row of their own over a status bar until the two were
folded into this one.

At the right are the buttons: first the selected row's -- move up, move down, forget a choice, show
the rule's file in its folder, each dimmed where it does not apply -- then the whole set's: sync now,
reload from disk, open the custom folder. The sync button is a cloud to fetch from, and while it fetches, the
cloud with its arrows turning inside it; its tooltip says where the last sync stands and nothing
else, rather than a line of its own saying so. The counts are on the tabs. Nothing
in the window is typed: a rule is written in a file.

A synced file naming an authority is reported only when that would have made a host one: the synced
copy of the built-in jsDelivr file names the hosts the built-in layer already does, and was reported
as a problem on every sync until this.

**A rule is moved up or down one place**, whatever layer it is in, and the move is written to the
custom layer's `order.toml` rather than to the rule's own file, a synced file being the sync's to
replace. Every rule of the kind is given a priority there, ten apart, in the order shown after the
move: rules of equal priority leave no room to put one between two others, and giving the moved one
a priority just past its neighbour's sent it past every rule that shared that priority. A rule that
arrives later, at the default priority, lands below the ordered ones until it is moved.

A choice is forgotten from the same window, and the next download from that domain is asked again.
