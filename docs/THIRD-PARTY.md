# Third-party dependencies

GIST itself is MIT-licensed (see `LICENSE`). This document lists every
third-party dependency shipped in a GIST build, per platform, with the
licence each was verified under.

## Rust core (`crates/`) — shared by every platform

The Rust workspace (`gist-model`/`gist-parse-*`/`gist-rsvp`/`gist-store`/
`gist-web`/`gist-imageprep`/`gist-core`/`gist-ffi`) compiles to
`GistCore.xcframework` on Apple platforms (ADR-001) and to the equivalent
native library on Windows; both native shells statically link it rather than
carrying any dependencies of their own for parsing/persistence/pacing. The
table below is every crate reachable from `gist-ffi` (the crate that's
actually built into the shipped library) via normal (non-dev, non-build)
dependency edges — i.e. `cargo tree -p gist-ffi -e normal`, deduplicated,
cross-referenced against `cargo metadata` for each crate's declared licence
expression.

**Methodology and caveats:**
- Regenerated 2026-10-04 (M6 R7) from `Cargo.lock` at this commit, using
  `cargo tree -p gist-ffi -e normal` (host target) joined against `cargo metadata` for each
  crate's declared licence/repository. 199 external crates (excluding the in-workspace
  `gist-*` crates themselves, which aren't third-party). Versions in earlier revisions of
  this table had drifted (e.g. `thiserror`, `uniffi*`, `encoding_rs`).
- `uniffi`'s proc-macro mode (ADR-001) pulls in a handful of crates that
  only run at compile time inside the proc-macro (e.g. `cargo_metadata`,
  `cargo-platform`, `camino`) and are not part of the shipped binary's
  machine code — they're included below anyway for completeness/disclosure
  rather than trying to prove exact byte-level link membership, and their
  licences are equally compliant either way.
- Cross-checked against `deny.toml`'s license allow-list: every licence
  expression below resolves to at least one allowed term (`MIT`,
  `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`,
  `BSD-3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`, `MPL-2.0`,
  `CDLA-Permissive-2.0`). Verified live: `cargo deny check bans licenses
  sources` → `bans ok, licenses ok, sources ok` (this dev environment's
  local `cargo-deny` 0.18.3 can run this check; `check advisories` needs
  the newer binary CI uses — see `CLAUDE.md`'s Security policies section).
- Some crates below appear twice at different versions (e.g. `syn`
  2.0.119/3.0.5, `hashbrown` 0.16.1/0.17.1) — expected, not a bug;
  `deny.toml`'s `multiple-versions = "warn"` flags but doesn't block this.

| Crate | Version | Licence | Repository |
|---|---|---|---|
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | <https://github.com/oyvindln/adler2> |
| aead | 0.6.1 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/traits> |
| aes | 0.9.2 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/block-ciphers> |
| aes-gcm | 0.11.1 | Apache-2.0 OR MIT | <https://github.com/RustCrypto/AEADs> |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | <https://github.com/dtolnay/anyhow> |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 | <https://github.com/bluss/arrayvec> |
| base64 | 0.22.1 | MIT OR Apache-2.0 | <https://github.com/marshallpierce/rust-base64> |
| bitflags | 2.13.1 | MIT OR Apache-2.0 | <https://github.com/bitflags/bitflags> |
| blake3 | 1.8.7 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | <https://github.com/BLAKE3-team/BLAKE3> |
| block-buffer | 0.12.1 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/utils> |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | <https://github.com/fitzgen/bumpalo> |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | <https://github.com/Lokathor/bytemuck> |
| byteorder | 1.5.0 | Unlicense OR MIT | <https://github.com/BurntSushi/byteorder> |
| byteorder-lite | 0.1.0 | Unlicense OR MIT | <https://github.com/image-rs/byteorder-lite> |
| bytes | 1.12.1 | MIT | <https://github.com/tokio-rs/bytes> |
| camino | 1.2.5 | MIT OR Apache-2.0 | <https://github.com/camino-rs/camino> |
| cargo-platform | 0.3.1 | MIT OR Apache-2.0 | <https://github.com/rust-lang/cargo> |
| cargo_metadata | 0.23.1 | MIT | <https://github.com/oli-obk/cargo_metadata> |
| cfb | 0.14.0 | MIT | <https://github.com/mdsteele/rust-cfb> |
| cfg-if | 1.0.4 | MIT OR Apache-2.0 | <https://github.com/rust-lang/cfg-if> |
| chardetng | 1.0.0 | Apache-2.0 OR MIT | <https://github.com/hsivonen/chardetng> |
| chrono | 0.4.45 | MIT OR Apache-2.0 | <https://github.com/chronotope/chrono> |
| cipher | 0.5.2 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/traits> |
| cmov | 0.5.4 | Apache-2.0 OR MIT | <https://github.com/RustCrypto/utils> |
| const-oid | 0.10.2 | Apache-2.0 OR MIT | <https://github.com/RustCrypto/formats> |
| constant_time_eq | 0.4.2 | CC0-1.0 OR MIT-0 OR Apache-2.0 | <https://github.com/cesarb/constant_time_eq> |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | <https://github.com/servo/core-foundation-rs> |
| cpubits | 0.1.1 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/utils> |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/utils> |
| crc32fast | 1.5.1 | MIT OR Apache-2.0 | <https://github.com/srijs/rust-crc32fast> |
| crypto-common | 0.2.2 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/traits> |
| cssparser | 0.37.0 | MPL-2.0 | <https://github.com/servo/rust-cssparser> |
| cssparser-macros | 0.7.1 | MPL-2.0 | <https://github.com/servo/rust-cssparser> |
| ctr | 0.10.1 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/block-modes> |
| ctutils | 0.4.2 | Apache-2.0 OR MIT | <https://github.com/RustCrypto/utils> |
| derive_more | 2.1.1 | MIT | <https://github.com/JelteF/derive_more> |
| derive_more-impl | 2.1.1 | MIT | <https://github.com/JelteF/derive_more> |
| digest | 0.11.3 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/traits> |
| directories | 6.0.0 | MIT OR Apache-2.0 | <https://github.com/soc/directories-rs> |
| dirs-sys | 0.5.0 | MIT OR Apache-2.0 | <https://github.com/dirs-dev/dirs-sys-rs> |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 | <https://github.com/yaahc/displaydoc> |
| dtoa | 1.0.11 | MIT OR Apache-2.0 | <https://github.com/dtolnay/dtoa> |
| dtoa-short | 0.3.5 | MPL-2.0 | <https://github.com/upsuper/dtoa-short> |
| ego-tree | 0.11.0 | ISC | <https://github.com/rust-scraper/ego-tree> |
| either | 1.18.0 | MIT OR Apache-2.0 | <https://github.com/rayon-rs/either> |
| encoding_rs | 0.8.42 | (Apache-2.0 OR MIT) AND BSD-3-Clause | <https://github.com/hsivonen/encoding_rs> |
| equivalent | 1.0.2 | Apache-2.0 OR MIT | <https://github.com/indexmap-rs/equivalent> |
| errno | 0.3.14 | MIT OR Apache-2.0 | <https://github.com/lambda-fairy/rust-errno> |
| fallible-iterator | 0.3.0 | MIT/Apache-2.0 | <https://github.com/sfackler/rust-fallible-iterator> |
| fallible-streaming-iterator | 0.1.9 | MIT/Apache-2.0 | <https://github.com/sfackler/fallible-streaming-iterator> |
| fastrand | 2.5.0 | Apache-2.0 OR MIT | <https://github.com/smol-rs/fastrand> |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 | <https://github.com/image-rs/fdeflate> |
| flate2 | 1.1.10 | MIT OR Apache-2.0 | <https://github.com/rust-lang/flate2-rs> |
| fnv | 1.0.7 | Apache-2.0 / MIT | <https://github.com/servo/rust-fnv> |
| foldhash | 0.2.0 | Zlib | <https://github.com/orlp/foldhash> |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 | <https://github.com/servo/rust-url> |
| fs-err | 3.3.1 | MIT OR Apache-2.0 | <https://github.com/andrewhickman/fs-err> |
| getopts | 0.2.24 | MIT OR Apache-2.0 | <https://github.com/rust-lang/getopts> |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | <https://github.com/rust-random/getrandom> |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | <https://github.com/rust-random/getrandom> |
| ghash | 0.6.0 | Apache-2.0 OR MIT | <https://github.com/RustCrypto/universal-hashes> |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | <https://github.com/rust-lang/hashbrown> |
| hashlink | 0.12.2 | MIT OR Apache-2.0 | <https://github.com/djc/hashlink> |
| heck | 0.5.0 | MIT OR Apache-2.0 | <https://github.com/withoutboats/heck> |
| html5ever | 0.39.0 | MIT OR Apache-2.0 | <https://github.com/servo/html5ever> |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/hybrid-array> |
| iana-time-zone | 0.1.65 | MIT OR Apache-2.0 | <https://github.com/strawlab/iana-time-zone> |
| icu_collections | 2.2.0 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| icu_locale_core | 2.2.0 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| icu_normalizer | 2.2.0 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| icu_normalizer_data | 2.2.0 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| icu_properties | 2.2.0 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| icu_properties_data | 2.2.0 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| icu_provider | 2.2.0 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| idna | 1.1.0 | MIT OR Apache-2.0 | <https://github.com/servo/rust-url/> |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT | <https://github.com/hsivonen/idna_adapter> |
| image | 0.25.10 | MIT OR Apache-2.0 | <https://github.com/image-rs/image> |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | <https://github.com/indexmap-rs/indexmap> |
| infer | 0.22.0 | MIT | <https://github.com/bojand/infer> |
| inout | 0.2.2 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/utils> |
| itertools | 0.13.0 | MIT OR Apache-2.0 | <https://github.com/rust-itertools/itertools> |
| itoa | 1.0.18 | MIT OR Apache-2.0 | <https://github.com/dtolnay/itoa> |
| libc | 0.2.189 | MIT OR Apache-2.0 | <https://github.com/rust-lang/libc> |
| libloading | 0.9.0 | ISC | <https://github.com/nagisa/rust_libloading/> |
| libsqlite3-sys | 0.38.2 | MIT | <https://github.com/rusqlite/rusqlite> |
| litemap | 0.8.3 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | <https://github.com/Amanieu/parking_lot> |
| log | 0.4.34 | MIT OR Apache-2.0 | <https://github.com/rust-lang/log> |
| markup5ever | 0.39.0 | MIT OR Apache-2.0 | <https://github.com/servo/html5ever> |
| maybe-owned | 0.3.4 | MIT OR Apache-2.0 | <https://github.com/rustonaut/maybe-owned> |
| memchr | 2.8.3 | Unlicense OR MIT | <https://github.com/BurntSushi/memchr> |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | <https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide> |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 | <https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide> |
| moxcms | 0.8.1 | BSD-3-Clause OR Apache-2.0 | <https://github.com/awxkee/moxcms.git> |
| multiversion_no_op | 1.0.0 | Apache-2.0 OR MIT | <https://github.com/hsivonen/multiversion_no_op> |
| new_debug_unreachable | 1.0.6 | MIT | <https://github.com/mbrubeck/rust-debug-unreachable> |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | <https://github.com/rust-num/num-traits> |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | <https://github.com/matklad/once_cell> |
| option-ext | 0.2.0 | MPL-2.0 | <https://github.com/soc/option-ext.git> |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | <https://github.com/Amanieu/parking_lot> |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | <https://github.com/Amanieu/parking_lot> |
| pdfium-render | 0.9.4 | MIT OR Apache-2.0 | <https://github.com/ajrcarey/pdfium-render> |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | <https://github.com/servo/rust-url/> |
| phf | 0.13.1 | MIT | <https://github.com/rust-phf/rust-phf> |
| phf_generator | 0.13.1 | MIT | <https://github.com/rust-phf/rust-phf> |
| phf_macros | 0.13.1 | MIT | <https://github.com/rust-phf/rust-phf> |
| phf_shared | 0.13.1 | MIT | <https://github.com/rust-phf/rust-phf> |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | <https://github.com/taiki-e/pin-project-lite> |
| piston-float | 1.0.1 | MIT | <https://github.com/pistondevelopers/float.git> |
| png | 0.18.1 | MIT OR Apache-2.0 | <https://github.com/image-rs/image-png> |
| polyval | 0.7.3 | Apache-2.0 OR MIT | <https://github.com/RustCrypto/universal-hashes> |
| potential_utf | 0.1.6 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| precomputed-hash | 0.1.1 | MIT | <https://github.com/emilio/precomputed-hash> |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | <https://github.com/dtolnay/proc-macro2> |
| pxfm | 0.1.30 | BSD-3-Clause OR Apache-2.0 | <https://github.com/awxkee/pxfm> |
| quick-xml | 0.42.0 | MIT | <https://github.com/tafia/quick-xml> |
| quote | 1.0.47 | MIT OR Apache-2.0 | <https://github.com/dtolnay/quote> |
| rand_core | 0.10.1 | MIT OR Apache-2.0 | <https://github.com/rust-random/rand_core> |
| ring | 0.17.14 | Apache-2.0 AND ISC | <https://github.com/briansmith/ring> |
| roxmltree | 0.21.1 | MIT OR Apache-2.0 | <https://github.com/RazrFalcon/roxmltree> |
| rusqlite | 0.40.2 | MIT | <https://github.com/rusqlite/rusqlite> |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT | <https://github.com/rust-lang/rustc-hash> |
| rustix | 1.1.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | <https://github.com/bytecodealliance/rustix> |
| rustls | 0.23.45 | Apache-2.0 OR ISC OR MIT | <https://github.com/rustls/rustls> |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 | <https://github.com/rustls/pki-types> |
| rustls-webpki | 0.103.15 | ISC | <https://github.com/rustls/webpki> |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | <https://github.com/bluss/scopeguard> |
| scraper | 0.27.0 | ISC | <https://github.com/rust-scraper/scraper> |
| selectors | 0.38.0 | MPL-2.0 | <https://github.com/servo/stylo> |
| semver | 1.0.28 | MIT OR Apache-2.0 | <https://github.com/dtolnay/semver> |
| serde | 1.0.229 | MIT OR Apache-2.0 | <https://github.com/serde-rs/serde> |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | <https://github.com/serde-rs/serde> |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | <https://github.com/serde-rs/serde> |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | <https://github.com/serde-rs/json> |
| serde_spanned | 1.1.1 | MIT OR Apache-2.0 | <https://github.com/toml-rs/toml> |
| servo_arc | 0.4.3 | MIT OR Apache-2.0 | <https://github.com/servo/stylo> |
| sha2 | 0.11.0 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/hashes> |
| simd-adler32 | 0.3.10 | MIT | <https://github.com/mcountryman/simd-adler32> |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 | <https://github.com/rusticstuff/simdutf8> |
| siphasher | 1.0.3 | MIT/Apache-2.0 | <https://github.com/jedisct1/rust-siphash> |
| smallvec | 1.16.0 | MIT OR Apache-2.0 | <https://github.com/servo/rust-smallvec> |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | <https://github.com/storyyeller/stable_deref_trait> |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | <https://github.com/nvzqz/static-assertions-rs> |
| string_cache | 0.9.0 | MIT OR Apache-2.0 | <https://github.com/servo/string-cache> |
| subtle | 2.6.1 | BSD-3-Clause | <https://github.com/dalek-cryptography/subtle> |
| syn | 2.0.119 | MIT OR Apache-2.0 | <https://github.com/dtolnay/syn> |
| syn | 3.0.5 | MIT OR Apache-2.0 | <https://github.com/dtolnay/syn> |
| synstructure | 0.13.2 | MIT | <https://github.com/mystor/synstructure> |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | <https://github.com/Stebalien/tempfile> |
| tendril | 0.5.1 | MIT OR Apache-2.0 | <https://github.com/servo/html5ever> |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | <https://github.com/dtolnay/thiserror> |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | <https://github.com/dtolnay/thiserror> |
| tinystr | 0.8.4 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| toml | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | <https://github.com/toml-rs/toml> |
| toml_datetime | 1.1.1+spec-1.1.0 | MIT OR Apache-2.0 | <https://github.com/toml-rs/toml> |
| toml_parser | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 | <https://github.com/toml-rs/toml> |
| toml_writer | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | <https://github.com/toml-rs/toml> |
| tracing | 0.1.44 | MIT | <https://github.com/tokio-rs/tracing> |
| tracing-attributes | 0.1.31 | MIT | <https://github.com/tokio-rs/tracing> |
| tracing-core | 0.1.36 | MIT | <https://github.com/tokio-rs/tracing> |
| typed-path | 0.12.3 | MIT OR Apache-2.0 | <https://github.com/chipsenkbeil/typed-path> |
| typenum | 1.20.1 | MIT OR Apache-2.0 | <https://github.com/paholg/typenum> |
| unicode-ident | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 | <https://github.com/dtolnay/unicode-ident> |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | <https://github.com/unicode-rs/unicode-segmentation> |
| unicode-width | 0.2.2 | MIT OR Apache-2.0 | <https://github.com/unicode-rs/unicode-width> |
| uniffi | 0.32.2 | MPL-2.0 | <https://github.com/mozilla/uniffi-rs> |
| uniffi_core | 0.32.2 | MPL-2.0 | <https://github.com/mozilla/uniffi-rs> |
| uniffi_internal_macros | 0.32.2 | MPL-2.0 | <https://github.com/mozilla/uniffi-rs> |
| uniffi_macros | 0.32.2 | MPL-2.0 | <https://github.com/mozilla/uniffi-rs> |
| uniffi_meta | 0.32.2 | MPL-2.0 | <https://github.com/mozilla/uniffi-rs> |
| uniffi_pipeline | 0.32.2 | MPL-2.0 | <https://github.com/mozilla/uniffi-rs> |
| universal-hash | 0.6.1 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/traits> |
| untrusted | 0.9.0 | ISC | <https://github.com/briansmith/untrusted> |
| ureq | 2.12.1 | MIT OR Apache-2.0 | <https://github.com/algesten/ureq> |
| url | 2.5.8 | MIT OR Apache-2.0 | <https://github.com/servo/rust-url> |
| utf16string | 0.2.0 | MIT OR Apache-2.0 | <https://github.com/getsentry/utf16string> |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT | <https://github.com/hsivonen/utf8_iter> |
| uuid | 1.26.1 | Apache-2.0 OR MIT | <https://github.com/uuid-rs/uuid> |
| vecmath | 1.0.0 | MIT | <https://github.com/pistondevelopers/vecmath.git> |
| web-time | 1.1.0 | MIT OR Apache-2.0 | <https://github.com/daxpedda/web-time> |
| web_atoms | 0.2.6 | MIT OR Apache-2.0 | <https://github.com/servo/html5ever> |
| webpki-roots | 0.26.11 | CDLA-Permissive-2.0 | <https://github.com/rustls/webpki-roots> |
| webpki-roots | 1.0.9 | CDLA-Permissive-2.0 | <https://github.com/rustls/webpki-roots> |
| winnow | 1.0.4 | MIT | <https://github.com/winnow-rs/winnow> |
| writeable | 0.6.4 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| yoke | 0.8.3 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| yoke-derive | 0.8.2 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| zerofrom | 0.1.8 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| zerofrom-derive | 0.1.7 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| zeroize | 1.9.0 | Apache-2.0 OR MIT | <https://github.com/RustCrypto/utils> |
| zerotrie | 0.2.5 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| zerovec | 0.11.8 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| zerovec-derive | 0.11.6 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| zip | 8.6.0 | MIT | <https://github.com/zip-rs/zip2> |
| zlib-rs | 0.6.7 | Zlib | <https://github.com/trifectatechfoundation/zlib-rs> |
| zmij | 1.0.23 | MIT | <https://github.com/dtolnay/zmij> |
| zopfli | 0.8.3 | Apache-2.0 | <https://github.com/zopfli-rs/zopfli> |
| zune-core | 0.5.3 | MIT OR Apache-2.0 OR Zlib | <https://github.com/etemesi254/zune-image> |
| zune-jpeg | 0.5.15 | MIT OR Apache-2.0 OR Zlib | <https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg> |

### PDF support (added M6 R1)

`gist-parse-pdf` adds `pdfium-render` 0.9.4 and its transitive crates (`libloading`, `chrono`, `itertools`, `vecmath`, ...); they are included in the table above (regenerated 2026-10-04 from `Cargo.lock`, which supersedes the separate sub-table R1 originally appended). All are permissive; no `deny.toml` change was needed.

### PDFium (prebuilt native library, `libpdfium.dylib`)

Not a Rust crate: a prebuilt universal2 (arm64 + x86_64) dynamic library from
<https://github.com/bblanchon/pdfium-binaries>, release `chromium/8076`, fetched by
`tools/fetch-pdfium.sh` (SHA-256 pinned, fails closed) into `artifacts/pdfium/` and
embedded in the app bundle at `Contents/Frameworks/libpdfium.dylib` (ADR-002 addendum).
No binary is committed to git.

| Component | Licence |
|---|---|
| PDFium | BSD-3-Clause |
| Abseil, LLVM libc | Apache-2.0 |
| AGG 2.3 (Anti-Grain Geometry) | Permissive AGG 2.3 licence (use/copy/modify/sell/distribute with the notice retained) |
| Little CMS (lcms2), simdutf, fast_float | MIT (texts in the release `licenses/` directory) |
| zlib | Zlib |
| libpng | PNG Reference Library License v2 |
| libjpeg-turbo | IJG + BSD-3-Clause + Zlib |
| OpenJPEG | BSD-2-Clause |
| ICU | Unicode / ICU licence |
| FreeType | FreeType Project License (BSD-style; requires the acknowledgement below) |

The release archive's `licenses/` directory carries the exact texts; they are
reproduced verbatim in the in-app Settings -> About -> Third-Party Notices screen
(`ThirdPartyNotices.txt`). Required FreeType acknowledgement:

> Portions of this software are copyright © The FreeType Project (www.freetype.org). All rights reserved.

The obligation for all of the above is notice reproduction, which the bundled
notices file satisfies. Note: AGG 2.3 (`agg23.txt`) was not in the pre-implementation
licence survey; it is permissive and MIT-compatible.

## Apple app (`apps/apple`)

The macOS/iOS SwiftUI shell has **zero direct third-party Swift dependencies**
— no Swift Package Manager packages, no CocoaPods, no vendored SDKs (confirmed
via `find apps/apple -iname Package.swift -o -iname Podfile`, no matches). It
links only Apple's own system frameworks (SwiftUI, AppKit, Vision for
on-device OCR per ADR-009, AVFoundation for read-aloud, Security for
Keychain-backed key custody per ADR-011) plus `GistCore.xcframework` (the
Rust core above). The in-app Settings → About → "Third-Party Notices" screen
(role R6, `apps/apple/Shared/LicensesView.swift`) bundles the Rust core table
above as `apps/apple/Shared/Resources/ThirdPartyNotices.txt`, generated from
this file — regenerate both together if the dependency tree changes.

## Fixtures and fonts

`fixtures/` (the parser test corpus) is entirely synthetic, generated
content — no third-party or copyrighted material is embedded, per
`fixtures/README.md`'s own header. No third-party fonts are bundled anywhere
in the repo (`find apps/apple -iname "*.ttf" -o -iname "*.otf"` — no
matches); typography uses only system fonts. Nothing to attribute here;
noted for completeness since an earlier draft of this document's own
changelog (`CLAUDE.md`'s M4 role description) assumed this file already
covered fixture/font provenance — it didn't, and there was nothing to add.

## Windows app (`apps/windows`) — NuGet packages

Versions are pinned in `apps/windows/Directory.Packages.props`. Licences below were read from each
package's `.nuspec` on nuget.org (and, for the file-based one, from the `license.txt` inside the
`.nupkg`), not inferred.

| Package | Version | Licence | Notes |
|---|---|---|---|
| System.Security.Cryptography.ProtectedData | 10.0.12 | MIT | SPDX expression in nuspec |
| CommunityToolkit.Mvvm | 8.4.2 | MIT | SPDX expression in nuspec |
| Microsoft.WindowsAppSDK | 2.5.1 | Microsoft Software License Terms, "Microsoft Windows App SDK" (proprietary, `license.txt` in package) | Not an OSI licence. Permits install/use to develop and test applications solely for Windows; review the redistribution terms in the package's `license.txt` before shipping (M4/MSIX release, ADR-017). Not covered by the Rust `cargo-deny` allow-list. |
| Microsoft.NET.Test.Sdk | 17.14.1 | MIT | Test-only, not shipped |
| xunit | 2.9.3 | Apache-2.0 | Test-only, not shipped |
| xunit.runner.visualstudio | 2.8.2 | Apache-2.0 | Test-only, not shipped |
| FlaUI.Core | 5.0.0 | MIT | `LICENSE.txt` in the package read; test-only (GIST.App.UITests), not shipped |
| FlaUI.UIA3 | 5.0.0 | MIT | `LICENSE.txt` in the package read; brings Interop.UIAutomationClient (transitive, unreviewed); test-only, not shipped |

Transitive NuGet packages are not listed here; `dotnet list package --include-transitive` in
`windows-build` CI covers vulnerabilities, and licences of transitive packages should be reviewed
when the first release build is produced. The Windows app also statically links the same Rust core
(above) via its native interop layer, not a separate dependency tree.
