# truesight

`cargo truesight` writes a Rust crate's API reference from the compiler's
rustdoc output, fails a check when the reference in your repository no
longer matches the code, and reports the API's health: how large it is, how
many names a user must learn for each task, and how much of it is
documented.

Every item, signature, count, and feature gate in the reference comes from
rustdoc JSON. Nobody types the item list, so it cannot drift: when the code
changes and the reference does not, `cargo truesight check` exits 1. Each
release adds its API changes to the book with no extra step.

## What it writes

| Output | Contents |
| --- | --- |
| `api/<package>.txt` | The public items and impls, one per line, with the full path, signature, and feature gate. |
| `<book>/reference/<package>/` | mdBook pages: an overview, one page per module with sections by kind, each type's methods, fields, variants, and trait implementations with short signatures that link to the crate's own types, the API changes in each release, one page per example, and a published copy of the item list. `sync` deletes any other `.md` or `.txt` file in this directory. It does not delete a symbolic link, or look inside a linked directory. |
| `<book>/llms.txt` | An index of the published pages in the llms.txt format. Its title is the package name, or with several crates the book's `title`, else the package names. |
| Blocks in Markdown files | Surface counts, the task table, features, the page list, examples, the health report, or a release's API changes, inside a README, `SUMMARY.md`, or `CHANGELOG.md`. |

A path that a `pub use` makes lists the item again, with its fields and
variants, or with the items that a trait declares. Impls, and the methods in
them, are listed once, under the path where the item is defined. When that
path is not public, they are listed under the item's shortest public path.

A line of `api/<package>.txt`:

```text
#[cfg(feature = "multipart")] pub fn reqwest::RequestBuilder::multipart(self, multipart: reqwest::multipart::Form) -> reqwest::RequestBuilder
```

## Requirements

- Rust and rustup. On its first run, truesight installs the nightly
  toolchain that writes the rustdoc JSON it reads, `nightly-2026-09-16`.
- mdBook, to build the book.

## Install

truesight is not on crates.io. Install it from the repository:

```sh
cargo install --locked --git https://github.com/manaforged/truesight
```

## Set up a workspace

Run this once at the workspace root:

```sh
cargo truesight init
```

`init` does the setup, then runs `sync`. It stops before it writes anything
when the workspace has no library crate. It stops the same way when a
crate's `<book>/reference/<package>/` directory already holds an `.md` or
`.txt` file, because `sync` deletes the files it did not write there.

1. It writes `truesight.toml` with every library crate in the workspace.
   Each crate gets its own tag pattern, `<package>-v{version}`, when the
   repository already has tags in that form, or when it has no `v{version}`
   tags and the crates are at different versions. Otherwise the crates share
   the default, `v{version}`.
2. It uses the first `book.toml` it finds in the root, `docs/`, `book/`, or
   `doc/`. When there is none, it creates an mdBook in `docs/` whose
   introduction page includes the README. It never overwrites `SUMMARY.md`
   or `introduction.md`.
3. It adds the API page list to the book's `SUMMARY.md`.
4. It adds an `## API` block to each crate README, before `## License`.

Commit what it writes. After that, run `cargo truesight check` before you
push. It exits 1 when a generated file is stale or a lint denies, and
`cargo truesight sync` fixes the stale files.

## Inspect and shape the API

In a crate, print a summary of the public API:

```sh
cargo truesight
```

```text
reqwest 0.13.5 · features: default, json, cookies, stream, multipart, socks, gzip, brotli, deflate, zstd, charset, http2 · nightly-2026-09-16 · rustdoc format 61
256 public items (31 types, 3 functions, 218 methods, 4 constants) at 258 paths; 2 are re-export aliases. 81 trait impl lines, 22 inherent impl lines. 7 modules, 0 re-export only.

reqwest                      mod      176
  Method                     use        1
  StatusCode                 use        1
  Url                        use        1
  Version                    use        1
  header                     use        1
  Body                       struct    17
```

| Command | Output |
| --- | --- |
| `cargo truesight show RequestBuilder::` | Every item whose path contains `RequestBuilder::`. |
| `cargo truesight show --kind trait --where` | Every public trait, and the file and line that define it. |
| `cargo truesight items` | The item list on disk as JSON: each crate's file, and each line with its kind and path. It builds nothing. |
| `cargo truesight diff` | API changes since the last `sync`. |
| `cargo truesight diff v0.1.0 --format changelog` | API changes since a git ref, as changelog bullets. |
| `cargo truesight lint` | Items exported at two paths, modules that only re-export, glob re-exports, and task map errors. |
| `cargo truesight unify` | A plan for one path per item: each extra path and the line that makes it, by file. |
| `cargo truesight migrate --from v0.1.0 tests` | Rewrites the paths in `tests` that moved since `v0.1.0`. |
| `cargo truesight health` | One report per crate: the surface, the prelude, the journeys, documentation coverage, and the lint findings. See [Health](#health). |

## Unify the API

`unify` plans the change to one path per public item. It edits nothing.
For each item at more than one path, it prints the item's own path (see
[Counts](#counts)) and every other path, with the `pub use`, glob `use`, or
`pub mod` line that makes that path. The lines are grouped by source file,
so you can edit one file at a time:

```text
src/lib.rs
  41: use adds `Circle`, another path to `shapes::Circle`
```

It also lists the modules that are re-export only, and each `pub use` that
puts a function next to a module with the same name that is not public.
Paths in the crate's `prelude` are expected; `unify` marks them as kept.

After the change, `migrate` rewrites the code that calls the crate:

```sh
cargo truesight migrate --from v0.1.0 src tests examples docs
```

It reads the item list committed at the ref and the current API, and
rewrites each path in the `.rs` and `.md` files under the given paths that
no longer exists:

- It reads a path from left to right. The first part of the path that no
  longer exists goes to where that item is now: the item with the same name
  and kind, preferably in a module on the old path. The rest of the path
  stays. After a type, the rest is a variant, field, or method, and stays as
  written.
- When a module on the path is no longer public and has no single new home,
  `migrate` drops it if the path without it exists.
- A `use` group with a moved item becomes one `use` per item. A group with
  `*`, `self`, nested braces, or comments stays as written.
- `use old::module::*;` changes when the module moved.
- In the package's own `src`, paths that start with `crate::` change too.

When no item fits, or more than one does, `migrate` leaves the path as
written and lists it with its file and line. A path that was not in the old
item list, such as a private path in the crate's own `src`, stays as written.

`--fix-imports <file>` reads compiler output and adds `use` lines. For each
name that rustc cannot find (E0412, E0422, E0425, E0433) in a file under the
given paths, and that has exactly one home, it adds `use <home>;` after the
file's leading inner attributes and comments, before its first `use`. When
the error is inside an inline `mod name { ... }` block, the line goes in that
block. Run `cargo fmt` after `migrate`.

## Releases

The API changes page lists each release. truesight reads the item list
committed at each release tag, `v<version>` by default. A tag counts only
when `HEAD` contains it and the item list is committed at it. truesight
compares consecutive releases:

- When you bump the version in `Cargo.toml`, the changes since the last tag
  appear under the new version. `check` makes that part of the bump commit.
- Tagging the release changes nothing, because the tag points at that
  commit.
- Changes after the last tag appear under `Unreleased` until the next bump.

A `changes` block in `CHANGELOG.md` shows one release, or the newest one
when it names none:

```markdown
<!-- truesight:changes 0.2.0 -->
<!-- /truesight -->
```

## Configuration

`truesight.toml` at the workspace root:

```toml
toolchain = "nightly-2026-09-16"
book = "docs"

[[crate]]
package = "reqwest"
features = ["json", "cookies", "stream", "multipart", "socks", "gzip", "brotli", "deflate", "zstd", "charset", "http2"]
gates = ["json", "cookies", "stream", "multipart"]
budget = 450

[lint]
duplicate-path = "warn"
```

| Key | Meaning |
| --- | --- |
| `toolchain` | The nightly that builds rustdoc JSON. |
| `book` | The mdBook source directory, the one that holds `SUMMARY.md`. Set it to write reference pages and `llms.txt`. |
| `markdown` | More Markdown files to fill. truesight always reads each crate's README, the book's `SUMMARY.md`, and `CHANGELOG.md` at the workspace root and in each crate directory. |
| `crate.features`, `crate.default-features` | The feature set to document. |
| `crate.gates` | Features to attribute to items. Each gate costs one more rustdoc build. With two or more gates, truesight also builds once with every gate off. |
| `crate.spine`, `crate.intent` | The item list and the task file. Defaults: `api/<package>.txt` and `api/<package>.toml`. |
| `crate.tag` | The release tag pattern. Default: `v{version}`. |
| `crate.budget` | The most public items the crate may have. Aliases and impl lines do not count; see [Counts](#counts). |
| `crate.prelude` | A module whose paths are expected aliases, such as `"prelude"`. The crate name can be left out. Default: none. |
| `crate.prelude-budget` | The number of names the prelude exports. `prelude-budget` denies more and fewer, so the budget only moves by an edit. |
| `crate.journey-prefix`, `crate.journeys` | The examples whose names start with the prefix are journeys. `journeys` maps each journey to its budget of names. See [Health](#health). |
| `crate.journey-ignore` | More words that the journey count skips. |
| `crate.docs` | Markdown files that document the API outside rustdoc. |
| `crate.doc-sections` | Headings in the `docs` files that list exactly the names of one module, such as `{ "Prelude" = "prelude" }`. `""` names the crate root. |
| `crate.lint` | Lint levels for this crate. They override `[lint]`. |

Without `truesight.toml`, `cargo truesight` documents the package in the
current directory with its default features.

## Tasks

The task file maps each job to the one call that does it. `check` resolves
every path in it against the code, so a renamed function fails the check.

```toml
[[task]]
name = "Add a bearer token"
call = "RequestBuilder::bearer_auth"
owner = "RequestBuilder::header_sensitive"
guide = "docs/auth.md"
```

`call` must be public. `owner` can be private. Paths can leave out the crate
name, and a path through any re-export of a type resolves. The overview page
and the `tasks` block render the file as a table.

## Markdown blocks

Put a pair of markers in a README, `SUMMARY.md`, or `CHANGELOG.md`:

```markdown
<!-- truesight:surface -->
<!-- /truesight -->
```

`sync` writes the block between them, and `check` fails when it is stale.
Markers inside code fences are left alone. The blocks are `surface`,
`tasks`, `features`, `pages`, `changes`, `examples`, and `health`. When truesight
documents more than one crate, name the package:
`<!-- truesight:tasks reqwest -->`.

`-p <package>` limits every command except `init` to one package. `init`
ignores it and sets up every library crate in the workspace. `sync -p` and
`check -p` leave blocks that name another package as written, and they
skip `llms.txt` when the book documents several crates.

## Counts

The `surface` block and the overview page start with one line that counts
the public items by kind, the paths that reach them, the re-export aliases
among those paths, the trait impl and inherent impl lines, the modules, and
the modules that are re-export only. A table with one row per module
follows it.

- A public item is one item that rustdoc documents: a type, function,
  method, field, variant, constant, static, or macro. Types include traits,
  type aliases, and associated types. A function is a method when its parent
  is a type or a trait. Modules, impl lines, and `pub use` lines for items of
  other crates are not items.
- An item that is exported at several paths counts once. Its own path is the
  path where it is defined, when that path is public. Otherwise it is the
  path with the fewest segments. Among paths with the same number of
  segments, truesight picks the one that shares the most leading segments
  with the definition, then the first in alphabetical order. A path in the
  `prelude` is an item's own path only when the item has no other path.
- Every other path to an item is a re-export alias.
- Trait impl lines are the `impl Trait for Type` lines and the associated
  types and constants under them. Inherent impl lines are the `impl Type`
  lines. The methods under an inherent impl are items.
- A module is re-export only when it has aliases and no item has its own
  path in it. A prelude is usually re-export only.

In the module table, Items counts the items whose own path is in the
module. Types, Functions, and Methods count some of those items by kind.
Aliases counts the alias paths in the module. A module that is re-export
only has `(re-export only)` after its name.

`budget` and the `over-budget` lint count public items, so an alias does
not count against the budget.

## Health

`cargo truesight health` prints one report per crate and exits like
`check`:

```text
calc 0.4.0
  surface     212 items in 5 modules: 21 types, 14 functions, 150 methods, 18 fields, 9 variants, 0 constants; 12 aliases; budget 220
  app tier    31 prelude names; budget 31
  journeys    3 journeys, 3 at budget: tour-hello 5/5, tour-sum 9/9, tour-plot 14/14
  documented  44 of 47 module-level items (93%): 38 by rustdoc, 6 by reference docs
  doc paths   0 reference rows name paths that are not public
  findings    3 deny, 0 warn
generated 14 files, 0 stale
```

- **Surface** is the [count](#counts) of public items.
- **App tier** counts the names the `prelude` exports: what a user gets
  from one glob import.
- **Journeys** measure how many library names a task costs. A journey is an
  example whose name starts with `journey-prefix`. truesight counts the
  distinct words in the example that name a public item of the crate. It
  skips Rust keywords, primitive types, `std` names such as `String` and
  `Some`, the words in `journey-ignore`, attributes, comments, strings, and
  every name the example declares: items, `let` and `for` bindings,
  parameters, fields, closure parameters, and match-arm bindings. When the
  example names another crate that truesight documents, that crate's names
  count too.
- **Documented** counts the items whose parent is a module: types,
  functions, constants, statics, and macros. An item is documented when it
  has rustdoc, or when its name appears in backticks in a table row of a
  `docs` file.
- **Doc paths** counts rows in a `docs` table with a `Name` column whose
  first cell names a path that is not public.

The `health` block writes the same report as a table.

## Lints

| Lint | Default | Finds |
| --- | --- | --- |
| `duplicate-path` | warn | One item exported at two paths. An item whose only other paths are in the `prelude` is not a finding. |
| `glob-reexport` | warn | A public `use` of `*`. |
| `reexport-only-module` | warn | A module that only re-exports items whose own path is in another module. The configured `prelude` is exempt. |
| `no-task` | allow | A public function or method that no task calls. |
| `unknown-path`, `unknown-owner`, `missing-guide`, `duplicate-task` | deny | A task that does not resolve. |
| `over-budget` | deny | More public items than `budget`. |
| `prelude-budget` | deny | A prelude with more or fewer names than `prelude-budget`. |
| `journey-budget` | deny | A journey above or below its budget, a journey without a budget, or a budget without a journey. |
| `undocumented` | allow | An item without rustdoc or a row in the `docs` files, or a module name missing under its `doc-sections` heading. |
| `stale-doc` | allow | A row under a `doc-sections` heading that names something the module does not export. |
| `unknown-doc-path` | allow | A row in a `docs` table with a `Name` column whose first cell names a path that is not public. |

Set `allow`, `warn`, or `deny` under `[lint]` for `duplicate-path`,
`glob-reexport`, `reexport-only-module`, `no-task`, `undocumented`,
`stale-doc`, and `unknown-doc-path`, or for one crate under `[crate.lint]`.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | The generated files are current and no lint denies. |
| 1 | A generated file is stale, or a lint denies. |
| 2 | truesight could not run. |

`show`, `diff`, `unify`, and `migrate` print and exit 0 unless truesight
could not run.

## Limits

- rustdoc JSON is unstable. truesight reads formats 59 to 61, which
  `nightly-2026-09-16` writes. Another nightly fails with the format it
  found.
- The changes page and `changes` blocks read the release tags in the history
  of `HEAD`. A shallow clone, which `actions/checkout` makes by default, does
  not have that history, so no earlier release counts: `check` reports
  `changes.md` and `changes` blocks as stale, and `sync` keeps the committed
  ones as they are. When a `changes` block names an earlier release, `check`
  exits 2 with ``no release `<version>` in the item list history``. `sync`
  and `check` print a note when the clone is shallow. Fetch the full history:
  set `fetch-depth: 0` on `actions/checkout`, or run
  `git fetch --unshallow --tags` in the shallow clone.
- `diff <ref>` reads the item list committed at that ref. When the ref has
  no `api/<package>.txt`, it exits 2 and names the file and the ref.
- The reference shows the first line of each item's doc comment. An item
  with no doc comment shows the names of the tasks that call it, or nothing.
  Guides stay in Markdown chapters, and tasks link to them.
- A gate marks the lines that disappear when truesight builds without that
  feature: `#[cfg(feature = "a")]` for one gate, `#[cfg(all(...))]` when an
  item needs several, and `#[cfg(any(...))]` when any one of them is enough.
  When a feature changes an item's signature, the list keeps the marked line
  and adds the other signature under `#[cfg(not(feature = "a"))]`.
- A deprecated item carries `#[deprecated]` after its gate marker.
- The item list leaves out auto-trait impls, and the copies of blanket impls,
  such as `impl<T> From<T> for T`, that rustdoc adds to each type. The
  crate's own blanket impls, such as `impl<T: Copy> Paint for T`, are listed.
  The list has each distinct line once. A trait impl is one
  `impl Trait for Type` line plus its associated types and constants. Its
  methods are not listed, so a task `call` names the trait's own method.
- Items marked `#[doc(hidden)]` are not listed, because rustdoc leaves them
  out of its JSON.
- rustdoc JSON leaves out private `use` items. An owner that is reachable
  only through a `pub(crate) use` needs the path where it is defined.
- Each feature set builds in its own directory under `target/truesight/`,
  so the first run builds every dependency once per gate.

## How the list is built

truesight runs `cargo rustdoc` with `--output-format json` on the pinned
nightly, and the `public-api` crate lists the public items from that JSON.
For each gate, truesight builds again without that feature and marks the
lines that disappear. With two or more gates, it also builds with every gate
off. When an item needs any one of several gates, it builds once more per
gate with only that gate on. The builds run in parallel. truesight does not
parse Rust source to build the list. Only `migrate` edits source text, with
the rules in [Unify the API](#unify-the-api).

## Contributing

truesight does not accept external pull requests until its API is more
stable. To report a bug or request a feature, open an
[issue](https://github.com/manaforged/truesight/issues).
