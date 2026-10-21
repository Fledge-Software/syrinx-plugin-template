<!-- SPDX-License-Identifier: MPL-2.0 -->

# Dependency audit

Every crate this template depends on, why it is here, its licence, its
maintenance and its security record. Syrinx's rule (`AGENTS.md` in Syrinx-app,
"Dependencies and licence") is that outside code comes in only under permissive
licences (MIT, Apache-2.0, BSD) or as whole MPL-2.0 files, never GPL, and that
nothing is added without checking maintenance, security and the licence of every
transitive crate. The template follows the same rule, because Syrinx's own
plugins are built from it. `cargo xtask licences` enforces the licence half on
every build, from `licences.toml`.

Audited on 2026-10-21 against `Cargo.lock` as committed, on Linux x86_64.

## Method

- **Graph.** `cargo metadata --format-version 1 --locked` lists 223 packages:
  the 4 workspace crates and **219 third-party crates**. This is the
  all-platform graph (it includes Windows- and macOS-only crates), so it
  over-counts any one machine: `cargo tree -e normal,build` resolves 188 for
  Linux and 183 for `x86_64-pc-windows-msvc` from the test host alone. The
  licence check runs on the larger set, so nothing escapes it.
- **Licences** are the SPDX strings in each crate's published metadata.
  ⚠️ The `LICENSE` files of the 212 crates that only run on the build machine
  were not read one by one; the 7 crates compiled into `plugin.wasm` have their
  licence files copied verbatim into `THIRD-PARTY-LICENSES` by
  `cargo xtask build`.
- **Security.** `cargo audit` 0.22.2 against the RustSec advisory database at
  commit `e6f94f3760d5e6c54c1b05f204002263bc025524` (2026-10-20): **0
  vulnerabilities, 0 warnings** (no unmaintained, unsound or yanked crates) in
  all 223 packages.
- **Overlap with Syrinx.** 145 of the 219 crates are already in Syrinx-app's
  `Cargo.lock` at the same version, and so were covered by its audits. 74 are
  new; they are listed below.

## Direct dependencies

| Crate | Pin | Used by | In `plugin.wasm`? | Licence | Why this one |
|---|---|---|---|---|---|
| `wit-bindgen` | `=0.63.0` | plugin | yes (runtime crate only) | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT (MIT elected) | Generates the guest side of the WIT world. Bytecode Alliance; the guest crate note 10 names |
| `serde` | `=1.0.229` | plugin, format, xtask | yes | MIT OR Apache-2.0 | Structs to and from JSON and TOML; same version as Syrinx |
| `serde_json` | `=1.0.151` | plugin, test host, xtask | yes | MIT OR Apache-2.0 | View message bodies and `cargo metadata`; same version as Syrinx |
| `toml` | `=1.1.8` (`parse`, `serde`, `std`) | format, xtask | no | MIT OR Apache-2.0 | Reads `syrinx-plugin.toml` and `licences.toml`; same version as Syrinx |
| `sha2` | `=0.11.0` | format | no | MIT OR Apache-2.0 | SHA-256 for the package list; same version as Syrinx |
| `wasmtime` | `=48.0.7`, features `runtime, cranelift, component-model, std, parallel-compilation, anyhow` | test host | no | Apache-2.0 WITH LLVM-exception | The runtime note 10 chose (decision 1), on the 48.x long-term-support line |
| `wasmtime-wasi` | `=48.0.7`, feature `p2` | test host | no | Apache-2.0 WITH LLVM-exception | WASI 0.2 with an empty context, which Rust's standard library needs |

Only the plugin crate's runtime dependencies reach the package. The test host,
the build tool and the formats crate run on the developer's or CI's machine and
are never shipped.

**Resolved wasmtime features** (`cargo tree -e features -i wasmtime`): `anyhow`,
`async`, `component-model`, `cranelift`, `parallel-compilation`, `runtime`,
`std`, plus internal `once_cell`, `wasmtime-jit-icache-coherence` and
`wasmtime-unwinder`. As note 10 predicted, `async` is switched on by
`wasmtime-wasi-io` even though nothing here runs asynchronously. Winch, the
pooling allocator, `component-model-async` and `wasi-http`, which carry most of
2026's advisories, are all off.

## What is compiled into `plugin.wasm`

Walking normal dependencies from `hello-plugin` for `wasm32-wasip2`, and
stopping at procedural macros (which run only at build time), gives seven
crates. These are what a reviewer of a Hello package is trusting.

| Crate | Version | Licence | Note |
|---|---|---|---|
| `itoa` | 1.0.18 | MIT OR Apache-2.0 | integer formatting for serde_json |
| `memchr` | 2.8.3 | Unlicense OR MIT | MIT elected |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | |
| `serde_core` | 1.0.229 | MIT OR Apache-2.0 | |
| `serde_json` | 1.0.151 | MIT OR Apache-2.0 | |
| `wit-bindgen` | 0.63.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | MIT elected |
| `zmij` | 1.0.23 | MIT | float formatting for serde_json, by serde's author |

## Licences that needed a decision

Counts across the 219 third-party crates: 109 `MIT OR Apache-2.0`, 25
`Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`, 12 `MIT`, 10
`Apache-2.0 OR MIT`, 6 more spellings of MIT-or-Apache (one also offering
BSD-2-Clause), and the cases below.
**No crate is GPL-only.**

- **`Apache-2.0 WITH LLVM-exception` alone: 32 crates**, all of the wasmtime and
  Cranelift family at the pinned versions: `cranelift-*` (13 crates, 0.135.7),
  `pulley-interpreter` and `pulley-macros` 48.0.7, `regalloc2` 0.15.2,
  `target-lexicon` 0.13.5, `wasmtime`, `wasmtime-environ`, `wasmtime-wasi`,
  `wasmtime-wasi-io` and eleven `wasmtime-internal-*` crates at 48.0.7, and
  `winx` 0.36.4. The owner approved this licence for the wasmtime and Cranelift
  family at the pinned versions on 2026-10-21 (note 10, decision 2); the
  exception only adds permissions to Apache-2.0. Note 10 counted 31 for a
  Windows-resolved graph; the 32nd here is `wasmtime-internal-jit-debug`, which
  appears in the all-platform metadata. `licences.toml` lists each by exact name
  and version rather than allowing the licence generally, so a new crate under
  it fails the check until reviewed.
- **Unicode-3.0: 18 crates** (`icu_*`, `litemap`, `potential_utf`, `tinystr`,
  `writeable`, `yoke`, `yoke-derive`, `zerofrom`, `zerofrom-derive`, `zerotrie`,
  `zerovec`, `zerovec-derive`), through `wasmtime-wasi` → `url` → `idna`. Every
  one is at exactly the version the owner approved for Syrinx on 2026-10-10
  (`docs/dev/integrated-preview-dependencies.md` in Syrinx-app). Test host only.
- **`unicode-ident`** is `(MIT OR Apache-2.0) AND Unicode-3.0`. A fresh resolve
  picked 1.0.27; the owner's approval names **1.0.26** only, so the lockfile is
  held at 1.0.26 (`cargo update -p unicode-ident --precise 1.0.26`), which
  wasmtime's `^1` requirement allows. Do not let `cargo update` move it.
- **`foldhash` 0.2.0** is Zlib, at the exact version the owner approved on
  2026-10-10. It arrives through `hashbrown` on the build machine only.
- **Elections recorded.** `memchr`, `termcolor` and `winapi-util` are
  `Unlicense OR MIT`: MIT is elected. `r-efi` 6.0.0 is
  `MIT OR Apache-2.0 OR LGPL-2.1-or-later`: MIT or Apache-2.0 is elected; it is a
  UEFI-target crate that is not compiled for Windows, Linux or WebAssembly here.
  `encoding_rs` 0.8.42 is `(Apache-2.0 OR MIT) AND BSD-3-Clause`, all permissive
  and already in Syrinx.
- **MPL-2.0: 4 crates**, the template's own workspace crates.

## Maintenance

- **wasmtime 48.0.7** (Bytecode Alliance) is the newest patch of the 48.x
  long-term-support line, published 2026-10-19 and supported for 24 months from
  2026-08-20. The project guarantees security backports to supported releases
  and publishes them often (note 10, section 1, "Maintenance"), so expect a pin
  bump every few weeks. A newer major, 50.0.0, exists; it is not used, because the
  owner chose the LTS line.
- **wit-bindgen 0.63.0** (Bytecode Alliance), published 2026-10-14; releases
  roughly monthly (0.61.1 on 2026-08-25, 0.62.0 on 2026-09-10). It pulls its own
  `wasmparser`, `wit-parser`, `wit-component` and `wasm-encoder` at 0.262.0,
  while wasmtime 48 uses 0.254.2 of the same crates, so both copies are in the
  lockfile. This costs build time only.
- **serde, serde_json, toml, sha2** (dtolnay, toml-rs, RustCrypto) are pinned to
  the versions already in Syrinx's lockfile rather than their newest patches
  (`serde_json` 1.0.154, `sha2` 0.11.1 exist), so the template adds no new review
  for them.
- **`tokio` 1.54.0** arrives through `wasmtime-wasi` (features `rt`,
  `rt-multi-thread`, `net`, `io-util`, `sync`, `time` and others). It is not in
  Syrinx's lockfile today, so Syrinx's own wasmtime audit (#27, #28) will meet it
  too. It is never started here; the test host is synchronous. tokio
  (tokio-rs, MIT) releases often, with patch releases on several supported
  lines at once (1.54.0 on 2026-10-19; 1.53.3 and 1.51.6 on 2026-10-13). RustSec
  lists five tokio advisories (RUSTSEC-2021-0072, 2021-0124, 2023-0001,
  2023-0005 and 2025-0023), all patched before 1.45; none affects 1.54.0, and
  `cargo audit` agrees. A fuller review belongs to Syrinx's own wasmtime audit
  in #27 and #28.

## Crates not already in Syrinx's lockfile (74)

`addr2line` 0.26.1, `ambient-authority` 0.0.2, `cap-primitives` 4.0.3, `cc`
1.9.0, `cobs` 0.3.0, `cpp_demangle` 0.5.1, the 13 `cranelift-*` crates 0.135.7,
`crc32fast` 1.5.3, `embedded-io` 0.4.0 and 0.6.1, `equivalent` 1.0.3,
`find-msvc-tools` 0.1.15, `fs-set-times` 0.20.3, `gimli` 0.33.0, `id-arena`
2.3.0, `io-extras` 0.19.0, `io-lifetimes` 2.0.4 and 3.0.1, `leb128fmt` 0.1.0,
`libc` 0.2.191, `mach2` 0.6.0, `maybe-owned` 0.3.4, `memfd` 0.6.6, `object`
0.39.1, `postcard` 1.1.3, `pulley-interpreter` and `pulley-macros` 48.0.7,
`regalloc2` 0.15.2, `rustix-linux-procfs` 0.1.1, `syn` 3.0.9, `target-lexicon`
0.13.5, `tokio` 1.54.0, `wasm-encoder`, `wasm-metadata`, `wasmparser`,
`wit-component` and `wit-parser` at both 0.254.2 and 0.262.0, `wasmprinter`
0.254.2, the 15 `wasmtime*` crates 48.0.7, `winx` 0.36.4 and the four
`wit-bindgen*` crates 0.63.0. All are MIT, Apache-2.0, a choice including one
of them, or the LLVM-exception family above.

## Tools used but not depended on

These run on a developer's machine or in CI and are not in `Cargo.lock`:
`wasm-tools` 1.262.0 (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT;
used once to parse the WIT and list the component's imports, not by the build),
`cargo-audit` 0.22.2 (Apache-2.0 OR MIT; not run by CI yet), Node.js (the
bridge tests use only its built-in test runner), Python 3's standard library
and `openssl` (signing), and `zip`. The interface in `ui/` has no JavaScript
dependencies.

## Every third-party crate

"In Syrinx" means the same name and version are already in Syrinx-app's
`Cargo.lock`. "Bundled" means compiled into `plugin.wasm`.

| Crate | Version | Licence | In Syrinx | Bundled |
|---|---|---|---|---|
| addr2line | 0.26.1 | Apache-2.0 OR MIT | no |  |
| allocator-api2 | 0.2.21 | MIT OR Apache-2.0 | yes |  |
| ambient-authority | 0.0.2 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | yes |  |
| arbitrary | 1.5.0 | MIT OR Apache-2.0 | yes |  |
| async-trait | 0.1.92 | MIT OR Apache-2.0 | yes |  |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | yes |  |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 | yes |  |
| block-buffer | 0.12.1 | MIT OR Apache-2.0 | yes |  |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | yes |  |
| bytes | 1.12.1 | MIT | yes |  |
| cap-primitives | 4.0.3 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| cc | 1.9.0 | MIT OR Apache-2.0 | no |  |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | yes |  |
| chacha20 | 0.10.2 | MIT OR Apache-2.0 | yes |  |
| cobs | 0.3.0 | MIT OR Apache-2.0 | no |  |
| const-oid | 0.10.2 | Apache-2.0 OR MIT | yes |  |
| core_detect | 1.0.0 | MIT/Apache-2.0 | yes |  |
| cpp_demangle | 0.5.1 | MIT OR Apache-2.0 | no |  |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 | yes |  |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 | yes |  |
| cranelift-assembler-x64 | 0.135.7 | Apache-2.0 WITH LLVM-exception | no |  |
| cranelift-assembler-x64-meta | 0.135.7 | Apache-2.0 WITH LLVM-exception | no |  |
| cranelift-bforest | 0.135.7 | Apache-2.0 WITH LLVM-exception | no |  |
| cranelift-bitset | 0.135.7 | Apache-2.0 WITH LLVM-exception | no |  |
| cranelift-codegen | 0.135.7 | Apache-2.0 WITH LLVM-exception | no |  |
| cranelift-codegen-meta | 0.135.7 | Apache-2.0 WITH LLVM-exception | no |  |
| cranelift-codegen-shared | 0.135.7 | Apache-2.0 WITH LLVM-exception | no |  |
| cranelift-control | 0.135.7 | Apache-2.0 WITH LLVM-exception | no |  |
| cranelift-entity | 0.135.7 | Apache-2.0 WITH LLVM-exception | no |  |
| cranelift-frontend | 0.135.7 | Apache-2.0 WITH LLVM-exception | no |  |
| cranelift-isle | 0.135.7 | Apache-2.0 WITH LLVM-exception | no |  |
| cranelift-native | 0.135.7 | Apache-2.0 WITH LLVM-exception | no |  |
| cranelift-srcgen | 0.135.7 | Apache-2.0 WITH LLVM-exception | no |  |
| crc32fast | 1.5.3 | MIT OR Apache-2.0 | no |  |
| crossbeam-deque | 0.8.8 | MIT OR Apache-2.0 | yes |  |
| crossbeam-epoch | 0.9.21 | MIT OR Apache-2.0 | yes |  |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | yes |  |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 | yes |  |
| crypto-common | 0.2.2 | MIT OR Apache-2.0 | yes |  |
| digest | 0.10.7 | MIT OR Apache-2.0 | yes |  |
| digest | 0.11.3 | MIT OR Apache-2.0 | yes |  |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 | yes |  |
| either | 1.19.0 | MIT OR Apache-2.0 | yes |  |
| embedded-io | 0.4.0 | MIT OR Apache-2.0 | no |  |
| embedded-io | 0.6.1 | MIT OR Apache-2.0 | no |  |
| encoding_rs | 0.8.42 | (Apache-2.0 OR MIT) AND BSD-3-Clause | yes |  |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | no |  |
| errno | 0.3.14 | MIT OR Apache-2.0 | yes |  |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 | no |  |
| fnv | 1.0.7 | Apache-2.0 / MIT | yes |  |
| foldhash | 0.2.0 | Zlib | yes |  |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 | yes |  |
| fs-set-times | 0.20.3 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| futures | 0.3.34 | MIT OR Apache-2.0 | yes |  |
| futures-channel | 0.3.34 | MIT OR Apache-2.0 | yes |  |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | yes |  |
| futures-io | 0.3.34 | MIT OR Apache-2.0 | yes |  |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 | yes |  |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | yes |  |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | yes |  |
| generic-array | 0.14.7 | MIT | yes |  |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | yes |  |
| gimli | 0.33.0 | MIT OR Apache-2.0 | no |  |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 | yes |  |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | yes |  |
| heck | 0.5.0 | MIT OR Apache-2.0 | yes |  |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 | yes |  |
| icu_collections | 2.3.0 | Unicode-3.0 | yes |  |
| icu_locale_core | 2.3.0 | Unicode-3.0 | yes |  |
| icu_normalizer | 2.3.0 | Unicode-3.0 | yes |  |
| icu_normalizer_data | 2.3.0 | Unicode-3.0 | yes |  |
| icu_properties | 2.3.0 | Unicode-3.0 | yes |  |
| icu_properties_data | 2.3.0 | Unicode-3.0 | yes |  |
| icu_provider | 2.3.1 | Unicode-3.0 | yes |  |
| id-arena | 2.3.0 | MIT/Apache-2.0 | no |  |
| idna | 1.1.0 | MIT OR Apache-2.0 | yes |  |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT | yes |  |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | yes |  |
| io-extras | 0.19.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| io-lifetimes | 2.0.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| io-lifetimes | 3.0.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| ipnet | 2.12.2 | MIT OR Apache-2.0 | yes |  |
| itertools | 0.14.0 | MIT OR Apache-2.0 | yes |  |
| itoa | 1.0.18 | MIT OR Apache-2.0 | yes | yes |
| leb128fmt | 0.1.0 | MIT OR Apache-2.0 | no |  |
| libc | 0.2.191 | MIT OR Apache-2.0 | no |  |
| libm | 0.2.16 | MIT | yes |  |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | yes |  |
| litemap | 0.8.3 | Unicode-3.0 | yes |  |
| log | 0.4.34 | MIT OR Apache-2.0 | yes |  |
| mach2 | 0.6.0 | BSD-2-Clause OR MIT OR Apache-2.0 | no |  |
| maybe-owned | 0.3.4 | MIT OR Apache-2.0 | no |  |
| memchr | 2.8.3 | Unlicense OR MIT | yes | yes |
| memfd | 0.6.6 | MIT OR Apache-2.0 | no |  |
| mio | 1.2.4 | MIT | yes |  |
| multiversion_no_op | 1.0.0 | Apache-2.0 OR MIT | yes |  |
| object | 0.39.1 | Apache-2.0 OR MIT | no |  |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | yes |  |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | yes |  |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | yes |  |
| postcard | 1.1.3 | MIT OR Apache-2.0 | no |  |
| potential_utf | 0.1.6 | Unicode-3.0 | yes |  |
| prettyplease | 0.3.0 | MIT OR Apache-2.0 | yes |  |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | yes |  |
| pulley-interpreter | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| pulley-macros | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| quote | 1.0.47 | MIT OR Apache-2.0 | yes |  |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | yes |  |
| rand | 0.10.3 | MIT OR Apache-2.0 | yes |  |
| rand_core | 0.10.1 | MIT OR Apache-2.0 | yes |  |
| rayon | 1.12.0 | MIT OR Apache-2.0 | yes |  |
| rayon-core | 1.13.0 | MIT OR Apache-2.0 | yes |  |
| regalloc2 | 0.15.2 | Apache-2.0 WITH LLVM-exception | no |  |
| rustc-demangle | 0.1.28 | MIT/Apache-2.0 | yes |  |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT | yes |  |
| rustix | 1.1.5 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | yes |  |
| rustix-linux-procfs | 0.1.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | yes |  |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | yes |  |
| semver | 1.0.28 | MIT OR Apache-2.0 | yes |  |
| serde | 1.0.229 | MIT OR Apache-2.0 | yes | yes |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | yes | yes |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | yes |  |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | yes | yes |
| serde_spanned | 1.1.2 | MIT OR Apache-2.0 | yes |  |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | yes |  |
| sha2 | 0.11.0 | MIT OR Apache-2.0 | yes |  |
| shlex | 2.0.1 | MIT OR Apache-2.0 | yes |  |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 | yes |  |
| slab | 0.4.12 | MIT | yes |  |
| smallvec | 1.16.3 | MIT OR Apache-2.0 | yes |  |
| socket2 | 0.6.5 | MIT OR Apache-2.0 | yes |  |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | yes |  |
| syn | 2.0.119 | MIT OR Apache-2.0 | yes |  |
| syn | 3.0.9 | MIT OR Apache-2.0 | no |  |
| synstructure | 0.14.0 | MIT | yes |  |
| target-lexicon | 0.13.5 | Apache-2.0 WITH LLVM-exception | no |  |
| termcolor | 1.4.1 | Unlicense OR MIT | yes |  |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | yes |  |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | yes |  |
| tinystr | 0.8.4 | Unicode-3.0 | yes |  |
| tokio | 1.54.0 | MIT | no |  |
| toml | 1.1.8+spec-1.1.0 | MIT OR Apache-2.0 | yes |  |
| toml_datetime | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | yes |  |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | yes |  |
| toml_writer | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 | yes |  |
| tracing | 0.1.44 | MIT | yes |  |
| tracing-attributes | 0.1.31 | MIT | yes |  |
| tracing-core | 0.1.36 | MIT | yes |  |
| typenum | 1.20.1 | MIT OR Apache-2.0 | yes |  |
| unicode-ident | 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 | yes |  |
| url | 2.5.8 | MIT OR Apache-2.0 | yes |  |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT | yes |  |
| version_check | 0.9.5 | MIT/Apache-2.0 | yes |  |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | yes |  |
| wasm-encoder | 0.254.2 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| wasm-encoder | 0.262.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| wasm-metadata | 0.254.2 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| wasm-metadata | 0.262.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| wasmparser | 0.254.2 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| wasmparser | 0.262.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| wasmprinter | 0.254.2 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| wasmtime | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| wasmtime-environ | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| wasmtime-internal-component-macro | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| wasmtime-internal-component-util | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| wasmtime-internal-core | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| wasmtime-internal-cranelift | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| wasmtime-internal-fiber | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| wasmtime-internal-jit-debug | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| wasmtime-internal-jit-icache-coherence | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| wasmtime-internal-unwinder | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| wasmtime-internal-versioned-export-macros | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| wasmtime-internal-wit-bindgen | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| wasmtime-wasi | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| wasmtime-wasi-io | 48.0.7 | Apache-2.0 WITH LLVM-exception | no |  |
| winapi-util | 0.1.11 | Unlicense OR MIT | yes |  |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | yes |  |
| windows-sys | 0.59.0 | MIT OR Apache-2.0 | yes |  |
| windows-sys | 0.60.2 | MIT OR Apache-2.0 | yes |  |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | yes |  |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | yes |  |
| windows-targets | 0.53.5 | MIT OR Apache-2.0 | yes |  |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | yes |  |
| windows_aarch64_gnullvm | 0.53.1 | MIT OR Apache-2.0 | yes |  |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 | yes |  |
| windows_aarch64_msvc | 0.53.1 | MIT OR Apache-2.0 | yes |  |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 | yes |  |
| windows_i686_gnu | 0.53.1 | MIT OR Apache-2.0 | yes |  |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 | yes |  |
| windows_i686_gnullvm | 0.53.1 | MIT OR Apache-2.0 | yes |  |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 | yes |  |
| windows_i686_msvc | 0.53.1 | MIT OR Apache-2.0 | yes |  |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | yes |  |
| windows_x86_64_gnu | 0.53.1 | MIT OR Apache-2.0 | yes |  |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | yes |  |
| windows_x86_64_gnullvm | 0.53.1 | MIT OR Apache-2.0 | yes |  |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | yes |  |
| windows_x86_64_msvc | 0.53.1 | MIT OR Apache-2.0 | yes |  |
| winnow | 1.0.4 | MIT | yes |  |
| winx | 0.36.4 | Apache-2.0 WITH LLVM-exception | no |  |
| wit-bindgen | 0.63.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no | yes |
| wit-bindgen-core | 0.63.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| wit-bindgen-rust | 0.63.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| wit-bindgen-rust-macro | 0.63.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| wit-component | 0.254.2 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| wit-component | 0.262.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| wit-parser | 0.254.2 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| wit-parser | 0.262.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |  |
| writeable | 0.6.4 | Unicode-3.0 | yes |  |
| yoke | 0.8.3 | Unicode-3.0 | yes |  |
| yoke-derive | 0.8.4 | Unicode-3.0 | yes |  |
| zerofrom | 0.1.8 | Unicode-3.0 | yes |  |
| zerofrom-derive | 0.1.8 | Unicode-3.0 | yes |  |
| zerotrie | 0.2.5 | Unicode-3.0 | yes |  |
| zerovec | 0.11.8 | Unicode-3.0 | yes |  |
| zerovec-derive | 0.11.6 | Unicode-3.0 | yes |  |
| zmij | 1.0.23 | MIT | yes | yes |
