<!-- SPDX-License-Identifier: MPL-2.0 -->

# Syrinx plugin template

This repository is the starting point for a **Syrinx plugin**. It holds a
complete, working, deliberately small plugin called **Hello**: one palette
command with a form, and one tab that lists the greetings the form makes. Use
the template to make a new repository, rename Hello, and replace its logic with
your own; the build, the tests, the package format and the release pipeline
already work.

Syrinx is a desktop web browser for Windows 11 with a vertical-tab sidebar.
Its own Notes and Forge are plugins too, built from this template and loaded
through exactly the same host, manifest, review prompt and grants as anyone
else's. That is on purpose: if the template is the path Syrinx's own plugins
take, it is the best-tested path, not the least-tested one.

> **Status, 2026-10-21.** Syrinx cannot load plugins yet. The host is being built
> in Syrinx-app [#28](https://github.com/Fledge-Software/Syrinx-app/issues/28)
> (runtime) and [#29](https://github.com/Fledge-Software/Syrinx-app/issues/29)
> (palette and tabs). Until then this template proves its plugin against a
> small **test host** in `test-host/` that follows the same rules. Everything in
> this README marked ⚠️ is a design that Syrinx's host has not yet confirmed.

## Contents

- [What a Syrinx plugin is](#what-a-syrinx-plugin-is)
- [Words used here](#words-used-here)
- [Making your own plugin from this template](#making-your-own-plugin-from-this-template)
- [Building and testing](#building-and-testing)
- [The manifest](#the-manifest)
- [Capabilities and grants](#capabilities-and-grants)
- [The interface: WIT](#the-interface-wit)
- [The tab's web view and the bridge](#the-tabs-web-view-and-the-bridge)
- [Palette forms](#palette-forms)
- [The package](#the-package)
- [Continuous integration](#continuous-integration)
- [Releasing and signing](#releasing-and-signing)
- [What is not possible yet](#what-is-not-possible-yet)
- [Repository layout](#repository-layout)
- [Licence](#licence)

## What a Syrinx plugin is

A Syrinx plugin is a program written in Rust and compiled to **WebAssembly**
that Syrinx runs inside a **sandbox**. The sandbox means the plugin can touch
nothing on your computer by itself: no files, no network, no other programs.
It reaches anything outside its own memory only by calling functions Syrinx
hands it, and Syrinx hands over a risky one (a folder, the network, a
terminal) only if the plugin asked for it in its manifest and the owner
granted it. This is the model Chrome extensions use, applied to compiled code.

A plugin can contribute three kinds of thing:

- **Commands** in Syrinx's command palette (Ctrl K). A command may show a
  **form** in the palette's preview pane; Syrinx draws the form from a
  description the plugin returns, so the plugin writes no interface code for it.
- **Tabs** that the plugin draws itself, in HTML, CSS and JavaScript, inside a
  web view that belongs to the plugin and nothing else.
- **Search feeds** the palette searches (Hello has none).

Why WebAssembly rather than an ordinary Rust library loaded into Syrinx? A
library would run with all of Syrinx's power and could read every file the
owner can. The owner decided on 2026-10-15 that every plugin, the built-in
ones included, runs sandboxed from the start; research note 10 explains the
choice of runtime and every rule below. It is in Syrinx-app at
[`docs/dev/background-10-wasm-plugin-runtime-draft.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/dev/background-10-wasm-plugin-runtime-draft.md)
(⚠️ the link resolves once that file is on Syrinx-app's `main`; it will also
become the wiki page `Background-10`).

## Words used here

- **WebAssembly (WASM)**: a portable bytecode. Syrinx's **runtime**, wasmtime,
  compiles it to machine code and runs it in the sandbox.
- **Host** and **guest**: Syrinx is the host; your plugin's module is the guest.
- **Component**: a WebAssembly file in the *component model* format, which
  passes typed values (strings, records, lists, variants) instead of raw bytes.
  `plugin.wasm` is a component.
- **WIT** (WebAssembly Interface Types): the language that describes what a
  component **imports** (what Syrinx offers the plugin) and **exports** (what
  the plugin offers Syrinx). Both sides' Rust code is generated from the same
  `.wit` file, so they cannot disagree about a type without the build or the
  load failing.
- **WASI 0.2**: standard WebAssembly interfaces (clocks, standard output and so
  on). Rust's standard library uses a few; Syrinx links them with an empty
  context, so they reach nothing.
- **Capability**: a named, scoped power, such as "this plugin's own settings" or
  "read and write one folder the owner picks". **Grant**: the owner's yes to a
  capability. Nothing is granted by default.
- **Manifest**: `syrinx-plugin.toml`, the plugin's description that Syrinx reads
  *without running any of the plugin's code*.
- **Plugin view**: Syrinx's own local web view that shows a plugin's tab. It is a
  different kind of web view from the page tabs that show the web.
- **Bridge**: the message channel between a plugin view's JavaScript and the
  plugin's module, through Syrinx, which checks every message.
- **The owner**: the person who uses Syrinx on their own computer and decides
  what each plugin may do. Not the plugin's author: as an author you *ask*;
  the owner *grants*.
- **WebView2**: Microsoft's embedded Edge browser, which Syrinx uses for page
  tabs and plugin views.
- **Origin**: the scheme, host and port a web page comes from, such as
  `https://syrinx-plugin.hello.invalid`. Browsers keep different origins' pages
  and storage apart, and Syrinx checks a message's origin before trusting it.
- **CSP** (Content Security Policy): a list a server sends with a page that
  says where the page may load scripts, styles, images and data from; the
  browser blocks everything else.
- **Nonce**: a random value made fresh for each page load. A CSP can allow
  only the style elements that carry it, which is how Syrinx's editor may add
  styles while injected ones are blocked.
- **Trap**: an unrecoverable stop inside the guest (a panic, running out of time
  or memory). Syrinx discards a trapped instance and starts a fresh one on next use.

## Making your own plugin from this template

1. On GitHub, choose **Use this template** to make a new repository. (The
   owner creates the template repository itself and decides its visibility;
   agents prepare its contents but never create or publish repositories.)
2. Choose a plugin id: lowercase letters, digits and hyphens, starting with a
   letter, for example `pomodoro`. It never changes once people install the
   plugin; a new id is a different plugin.
3. Rename Hello. Search the repository for `hello` and replace it:
   - `syrinx-plugin.toml`: `id`, `name`, `description`, `authors`,
     `repository`, and every contributed id (`hello.greet` becomes
     `pomodoro.start`; contributed ids must start with the plugin id).
   - `plugin/Cargo.toml`: the package `name` (for example `pomodoro-plugin`).
     The build tool reads the name from this file, so nothing in `xtask/`
     changes.
   - `plugin/src/greeter.rs`: the command and tab ids at the top.
   - `ui/index.html`: the title and the text that names "Say hello".
4. Refresh `Cargo.lock` for the new package name and commit it:

   ```
   cargo update --workspace
   ```

   This rewrites only the workspace's own entries. **Never run a bare
   `cargo update`**: it moves every dependency to its newest compatible
   version, including `unicode-ident`, which is held at 1.0.26 because that is
   the version the owner approved; the licence check then fails. Every cargo
   call in `cargo xtask` uses `--locked`, so a stale lockfile stops the build
   with "cannot update the lock file" rather than changing it silently.
5. Replace the logic in `plugin/src/greeter.rs` and the tab in `ui/`. Keep
   `plugin/src/lib.rs` as the thin layer that hands each WIT export to your
   logic. Then rewrite `test-host/tests/contract.rs` for your plugin: its `GREET`
   constant, the form values and the Hello-specific assertions describe Hello.
   Keep its refusal tests (tampered package, undeclared methods, foreign origin,
   denied and revoked capabilities), which test the host's rules. The package
   folder is found from your manifest's id and version, so that needs no change.
   Run `cargo xtask check`; when it passes, your plugin builds, its package is
   valid, and the test host can load and drive it.
6. Use example values only: `example.com`, `.test` and `.invalid` addresses and
   made-up names. Never put real personal data, keys or tokens in the
   repository, its tests or its fixtures.

## Building and testing

You need:

- **Rust through [rustup](https://rustup.rs)**. The exact toolchain, with the
  `wasm32-wasip2` target, rustfmt and Clippy, is named in `rust-toolchain.toml`;
  `rustup toolchain install` in this folder installs it explicitly, as CI does,
  and rustup otherwise installs it on the first build.
- **[Node.js](https://nodejs.org) 20 or later**, for the interface's tests.
- **Python 3 and `openssl`**, only to sign releases.

The first build downloads about 220 crates from crates.io and compiles
wasmtime for the test host, which takes several minutes (about 3 on a 4-core
machine); later builds take seconds.

The build tool is a Rust program in `xtask/`, run through a cargo alias
(`.cargo/config.toml`). It was chosen over a `justfile` or a shell script so the
same commands work on Windows and Linux with nothing but Rust installed.

| Command | What it does |
|---|---|
| `cargo xtask build` | Builds `plugin.wasm` for `wasm32-wasip2` and assembles the package folder `dist/hello-0.1.0/` with its file list |
| `cargo xtask test` | Builds, then runs the unit tests, the contract tests and the bridge's JavaScript tests |
| `cargo xtask check` | `cargo fmt --check`, Clippy with warnings denied (native and `wasm32-wasip2`), the UI rules, the licence check, the build and all tests: what CI runs, except the reproducibility build, the signature self-test and the Windows build |
| `cargo xtask licences` | Checks every crate in `Cargo.lock` against `licences.toml` |
| `cargo xtask repro` | Builds twice, from two copies of the source in different folders, and requires identical bytes |
| `cargo xtask check-tag v0.1.0` | Refuses a release tag that is not `v` plus the manifest's version |

There are three layers of tests, and each catches something the others cannot:

- **Unit tests** (`plugin/src/greeter.rs`) test the plugin's logic on your own
  machine, with a fake host, in milliseconds. `HostApi` is the trait that makes
  this possible: the real implementation calls Syrinx, the test one records calls.
- **Contract tests** (`test-host/tests/contract.rs`) load the *built package*
  into a wasmtime test host generated from the same WIT, and drive the
  lifecycle: activate, run the command, open the tab, send it bridge messages,
  close and reopen it, submit the form, deactivate. They also check the refusals:
  a capability that is not granted returns `denied`; a capability the manifest
  does not request cannot be granted; undeclared commands, view methods,
  oversized or wrongly addressed messages never reach the plugin; a changed,
  extra or precompiled file in the package is refused.
- **Bridge tests** (`tests/ui/bridge.test.mjs`) test `ui/bridge.js` against a
  fake `chrome.webview` with Node's built-in test runner.

`cargo test` alone runs the unit tests but fails the contract tests until a
package exists; use `cargo xtask test`, which builds the package first.

## The manifest

`syrinx-plugin.toml` tells Syrinx everything it must know **before the plugin
runs**: who the plugin is, which WIT world it targets, which commands and tab
types it contributes, and which capabilities it asks for. Syrinx starts a plugin
only on first use, so the palette must be able to list its commands from the
manifest alone. The rule is: *anything Syrinx needs before a plugin runs is
declared in the manifest; the plugin's exports only do things.*

Syrinx refuses a manifest, with a named error, if its `schema` or `world` is not
supported, a contributed id lacks the plugin's prefix or appears twice, a view
method is malformed, an unknown field appears, or a capability has an unknown
kind, no reason, or is a full-trust kind without `trust = "full"`. The same
checks run here, in `format/src/manifest.rs`, every time you build.

Two fields deserve a word. `ui-methods` on a tab type lists the only methods the
tab's JavaScript may call; anything else is refused before the plugin sees it,
so a typo or an attack costs no plugin call. `form = true` on a command means
the palette shows the plugin's form when the command is chosen.

## Capabilities and grants

**Hello asks for nothing,** so installing it asks the owner to grant nothing.
The manifest ends with a commented example of a request:

```toml
[[capabilities]]
kind = "storage"                   # this plugin's own namespaced settings
reason = "Remember the last name you greeted."
required = false                   # optional: asked for at first use, can be refused
```

Every request names a `kind`, gives a `reason` the owner reads in plain words,
and says whether the plugin works without it. How the owner sees them (⚠️ the
design approved in note 10, not yet built in Syrinx):

- **At install**, a review sheet lists each capability with its reason.
  Required ones are on; declining one means the plugin is not installed.
  Optional ones start off.
- **At first use**, an optional capability is asked for only after the owner's
  own action, such as a click. A folder is always chosen by the owner in the
  Windows folder picker; a plugin never names a path.
- **At any time**, the owner can revoke a grant. Later calls return `revoked`
  (a capability that was never granted returns `denied`), and the plugin must
  carry on.
- **An update that asks for more** is held inactive, with the old version still
  running, until the owner accepts the difference.

Every host function checks its own grant on every call. A plugin should
therefore treat `denied` as an ordinary answer, not a crash: Hello's form still
greets without `storage`, it just cannot remember the name (see
`Greeter::remember_name`). The contract tests prove both paths, by loading
Hello with its own manifest (denied) and with the request uncommented (granted,
then revoked).

Kinds the note names: `storage`, `folder`, `drafts`, `index`, `open-external`
and `network` (sandboxed), `parse` (Tree-sitter), and `terminal`, `git` and
`language-servers`. The last three start programs as the owner, which no sandbox
can follow, so they are **full trust**: a request must say `trust = "full"`, the
owner sees them under "Can run programs on your computer with your
permissions", and third-party plugins cannot get them in v1. Only `storage`
exists as a host function in WIT 0.1.0 (`settings-get` and `settings-set`); the
others are designed in Syrinx #28 and the Notes and Forge work.

## The interface: WIT

`wit/plugin.wit` is the `syrinx:plugin@0.1.0` world from research note 10, with
the changes listed in `docs/wit-changes.md`. In short:

- The plugin **imports** `types` (shared records and the `plugin-error` variant)
  and `host` (logging, its own settings, opening a view, setting a tab's title
  and badge, posting an event to its view, the clock).
- It **exports** `lifecycle` (activate on first use, deactivate), `commands`
  (`run`), `forms` (`form-for`, `validate`, `submit`), `views` (`opened`,
  `message`, `closed`), `search-feed` (Hello has no feed and answers
  `not-found`) and `events` (results of long-running form submissions).

Everything Syrinx reads or draws is a typed record; only a view's message body
and a plugin's own settings are opaque JSON or bytes. Every export is an
interface, never a bare function, so the version travels in the names inside
`plugin.wasm` (`syrinx:plugin/commands@0.1.0`) and the host can refuse a plugin
built for a world it does not support. Calls are synchronous; each runs under a
time limit (an *epoch deadline*) and a memory cap, and a plugin that exceeds one
traps rather than freezing Syrinx.

`plugin/src/lib.rs` generates the guest's Rust bindings with `wit-bindgen` and
hands each export to `plugin/src/greeter.rs`. `test-host/src/lib.rs` generates
the host's bindings from the same file with wasmtime's `bindgen!`.

## The tab's web view and the bridge

The Greetings tab is the HTML, CSS and JavaScript in `ui/`. Syrinx serves these
files from the verified package at an origin of the plugin's own,
`https://syrinx-plugin.hello.invalid/` (⚠️ the URL form is read from wry's
source, not yet run). `.invalid` is a top-level domain reserved never to
resolve, so if anything ever went wrong the request would fail rather than reach
a real server. The view sits in a private, in-memory browser profile, so a
plugin cannot keep state in the browser: its state lives in its Rust module.

Syrinx sends a strict **Content Security Policy** with every page it serves:

```
default-src 'none'; script-src 'self'; style-src 'self' 'nonce-<per-load>';
img-src 'self' data: blob:; font-src 'self'; connect-src 'self'; worker-src 'self';
frame-src 'none'; child-src 'none'; object-src 'none';
base-uri 'none'; form-action 'none'; frame-ancestors 'none'
```

That means: scripts and styles only from the plugin's own files (no inline
`<script>`, no `style=""`, no `eval`), and **no network from the view, ever**,
even for a plugin granted network access: network use goes through the module's
host functions, where the grant is checked. `cargo xtask build` refuses `ui/`
files that would break these rules (`xtask/src/ui_lint.rs`), so you find out at
build time rather than from a blank tab. The view also shows plugin text with
`textContent`, never `innerHTML`: it draws data it does not trust, and a script
injected there must gain nothing.

**The bridge.** The view talks only to its own plugin, through Syrinx, using
WebView2's built-in `chrome.webview.postMessage`. Syrinx injects nothing into the
view; `ui/bridge.js` is the plugin's own helper. Messages are JSON envelopes:

```json
{ "v": 1, "kind": "request", "id": 42, "method": "list-greetings", "body": {} }
{ "v": 1, "kind": "response", "id": 42, "ok": true, "body": { "greetings": [] } }
{ "v": 1, "kind": "response", "id": 42, "ok": false, "error": { "kind": "not-found", "message": "…" } }
{ "v": 1, "kind": "event", "topic": "greeting-added", "body": { "text": "Hello, Ada!", "at": 0 } }
{ "v": 1, "kind": "cancel", "id": 42 }
```

`v` is the bridge's version. `id` is chosen by the view and unique among its
pending requests, so replies can arrive in any order. Before any plugin code
runs, Syrinx checks, in this order: the message came from this plugin's origin;
the view is current (not closed or reloaded since); it is at most 1 MiB; it is a
well-formed envelope; its method is in the manifest's `ui-methods`; its id is
not already pending and the view's queue is not full. A refused message gets no
reply, and `bridge.js` rejects it after its timeout (10 s by default) and sends
a `cancel`. The plugin receives an accepted request as `views.message` with a
request id the host allocates, and its reply must be valid JSON. The plugin
pushes updates with `host.post-view-event`, which arrive as `event` envelopes.

```js
import { createBridge } from "./bridge.js";
const bridge = createBridge();
const { greetings } = await bridge.request("list-greetings");
bridge.on("greeting-added", (greeting) => { /* draw it */ });
```

## Palette forms

A form is *declarative*: `forms.form-for` returns a `form-spec` (a title, a
submit label and fields of kind `text`, `multiline`, `number`, `money`, `choice`
or `toggle`), and Syrinx draws it in the palette with its own interface toolkit.
Hello's form has a text field, a choice and a toggle. The field values stay in
Syrinx until Enter. Syrinx checks `required` and lengths; `forms.validate` is the
plugin's own check, returning every problem at once; `forms.submit` acts.

Each submission carries a `submission-id` the host allocates, so pressing Enter
twice cannot greet twice (Hello also remembers recent ids). `submit` answers
`finished` with an outcome, `invalid` with field errors (nothing happened), or
`pending` with a request id whose outcome arrives later through
`events.deliver`. An outcome of `uncertain` means "the request reached the
service but completion is unknown", and Syrinx never resubmits it.

## The package

`cargo xtask build` writes `dist/hello-0.1.0/`:

```text
hello-0.1.0/
  syrinx-plugin.toml       the manifest
  plugin.wasm              the component (never precompiled native code)
  ui/index.html, ui/app.js, ui/bridge.js, ui/app.css
  LICENSE
  THIRD-PARTY-LICENSES     every crate compiled into plugin.wasm, with its licence text
  package                  the canonical file list
  package.sig              its signature (added by the release workflow)
```

`package` is the list Syrinx verifies, in the same shape as Syrinx's own update
manifest (`docs/update-format.md` in Syrinx-app):

```text
syrinx-plugin-package 1
id hello
version 0.1.0
world syrinx:plugin/plugin@0.1.0
file <sha256> <size> LICENSE
file <sha256> <size> plugin.wasm
...
```

It has exactly one valid byte form (LF line endings, single spaces, files
sorted by path), and the reader re-renders what it parsed and requires the same
bytes, so two readers can never see different contents in one signed file.
Paths must be safe on Windows: components separated by `/`, each made of ASCII
letters, digits, `.`, `_` and `-` only, no
`..`, no `:` (alternate data streams), no device names like `CON`, and no two
paths that differ only in letter case. Syrinx installs the folder only if the
signature verifies, every listed file matches its size and hash, and no other
file is present. It is a folder, not an archive, so Syrinx needs no archive
parser; Windows' "Extract All" unpacks the release zip. `format/src/package.rs`
writes and reads the list.

## Continuous integration

`.github/workflows/ci.yml` runs on every push and pull request (⚠️ the owner
approved these workflows as described in note 10 on 2026-10-21; they have not yet
run on GitHub):

- `cargo xtask check` on Ubuntu: format, Clippy, UI rules, licences, the build,
  unit, contract and bridge tests;
- the signature format's self-test;
- `cargo xtask repro`, two builds from two folders that must be identical;
- a Windows build, whose `plugin.wasm` digest is compared with Linux's. Whether
  the two systems give identical bytes is not yet known, so a difference is a
  warning for now.

Every third-party action is pinned by its full commit SHA, with the tag in a
comment. The licence check enforces `licences.toml`: MIT, Apache-2.0, BSD and
MPL-2.0 for any crate, no GPL, and a short list of exact-version exceptions the
owner approved for Syrinx (see `docs/dependency-audit.md`). A crate that offers
a choice (`MIT OR Apache-2.0`) passes when one choice is allowed. The
interface has no JavaScript dependencies at all, so there is nothing to check
there; if you add one, add it to the audit and to `THIRD-PARTY-LICENSES`.

## Releasing and signing

1. Set `version` in `syrinx-plugin.toml`, commit, and push a tag `v` plus that
   version (`v0.1.0`). `release.yml` runs only on such a tag and refuses one that
   does not match.
2. It runs all of CI first, then builds the package, **signs** the `package`
   list, zips the folder, writes `SHA256SUMS`, **attests** the build's provenance
   and publishes a GitHub release with the zip and `SHA256SUMS`.

**Signing** proves who published a package. The signature, `package.sig`, is 76
bytes: the magic `SPP1`, an 8-byte key id (the first bytes of SHA-256 over the
public key), and an ECDSA P-256 signature over the `package` bytes, the format
Windows' built-in cryptography checks. Only the list is signed; each file is
bound to it by its hash. Syrinx records the key the first time a plugin is
installed (*trust on first use*) and refuses a later update signed by a
different key. `scripts/package-signature.py` signs and verifies with Python's
standard library and `openssl`.

The release needs one repository secret, **`SYRINX_PLUGIN_SIGNING_KEY`**: an
ECDSA P-256 private key in PEM form. The repository's owner creates it on a
machine they control, outside any agent session, and stores it as an encrypted
Actions secret; nobody else sees it, and it is never committed. This template
ships no key and the workflow stops if the secret is missing: there are no
unsigned releases. The workflow uses a GitHub *environment* named `release`, so
the owner can require an approval before anything is signed. ⚠️ Whoever controls
the repository's GitHub account or workflows can sign; that is the same trade-off
the owner accepted for Syrinx's own release key on 2026-10-14.

**Attestation** records, signed by GitHub through Sigstore, that `plugin.wasm`
and the zip were built by this repository's release workflow at the tagged
commit. Because the attest step runs inline in `release.yml`, this reaches SLSA
Build Level 2; note 10 suggests moving it into a reusable workflow for Level 3,
which is left for later. Anyone can check it before installing:

```
gh attestation verify syrinx-plugin-hello-0.1.0.zip -R <owner>/<repository>
```

⚠️ Attestations work on public repositories on every GitHub plan, but on private
repositories only with GitHub Enterprise Cloud; on a private repository without
it, the attestation step fails. The owner chose public plugin repositories
(note 10, decision 14).

**Reproducibility** lets a reviewer rebuild a release and compare: check out the
tag, run `cargo xtask build`, and compare `plugin.wasm`'s SHA-256 with the one
in `SHA256SUMS`. The build rewrites source paths (`--remap-path-prefix`), strips
symbols and uses one code-generation unit so the bytes do not depend on the
machine. A plain `cargo build` without those path rewrites gives different bytes.

## What is not possible yet

- **Loading in Syrinx.** Syrinx has no plugin host yet (#28, #29). The
  "Install plugin from folder…" flow, the review sheet, grants, the plugin view
  and the palette form are designs; the test host here only imitates their rules.
- **Capabilities beyond `storage`.** Folders, watching, drafts, indexing,
  opening links, network and the full-trust kinds have no WIT interfaces yet.
- **Home widgets.** The `widgets` interface is defined but gated and not
  exported (`docs/wit-changes.md`, change 3); it waits for Syrinx #31.
- **The shared editor.** Notes and Forge will use a CodeMirror editor Syrinx
  provides; a plugin cannot put its own editor extension into it in v1.
- **Async calls.** WIT 0.1.0 is synchronous; streams for terminals and language
  servers come in a later track.
- **Cross-system reproducibility** (Linux against Windows) is measured by CI but
  not yet confirmed.

## Repository layout

```text
syrinx-plugin.toml   the manifest
wit/plugin.wit       the syrinx:plugin@0.1.0 world
plugin/              the plugin: lib.rs (WIT glue), greeter.rs (logic and unit tests)
ui/                  the Greetings tab: index.html, app.js, bridge.js, app.css
format/              the manifest and package-list formats (shared by xtask and test-host)
test-host/           the wasmtime test host and the contract tests
xtask/               the build tool: build, test, check, licences, repro, check-tag
tests/ui/            the bridge's Node tests
scripts/             package-signature.py (sign, verify, self-test)
licences.toml        the licence policy cargo xtask licences enforces
docs/                dependency-audit.md, wit-changes.md
.github/workflows/   ci.yml, release.yml
```

## Licence

The template is licensed under the Mozilla Public License 2.0, the same as
Syrinx (see `LICENSE`). MPL-2.0 applies file by file: changes to these files stay
under MPL-2.0, while files you add may use another licence. ⚠️ Issue #30 notes
that a permissive licence might suit a template better, since its files are
copied into other people's plugins; the owner chose MPL-2.0 for now.
