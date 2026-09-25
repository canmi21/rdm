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

Rules carry a priority, and the merge orders every rule of every layer by it, so a user's rule can
be placed anywhere among the others rather than only before or after all of them. **Where several
rules match one address, the one with the highest priority is used whole** and the rest not at all:
a user changes what an entry does with one rule of their own, rather than by writing exclusions
against every rule whose mirrors would otherwise be merged in.

The synced layer is not signed, for now. What a tampered rule can do is limited by the next section:
a mirror is only used with a checksum from the source, and the list of mirrors trusted to answer
for a source cannot be changed by a sync, so the worst a changed rule does is send a download to a
mirror whose bytes then fail the check.

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

## A window for the rules

A window of its own lists the merged rules with the layer each came from, and is where they are
ordered and where the custom layer is edited.
