# GIST Fuzz Targets

Fuzz harnesses for the GIST parser crates using `cargo-fuzz` (libFuzzer backend).

## Prerequisites

`cargo-fuzz` requires nightly Rust:

```sh
rustup install nightly
cargo install cargo-fuzz --locked
```

## Running a target

From the workspace root:

```sh
cargo fuzz run fuzz_parse_epub -- -max_len=1048576
cargo fuzz run fuzz_parse_docx -- -max_len=1048576
cargo fuzz run fuzz_parse_txt  -- -max_len=1048576
cargo fuzz run fuzz_web_extract -- -max_len=65536
```

Or pin nightly without changing the default toolchain:

```sh
rustup run nightly cargo fuzz run fuzz_parse_epub -- -max_len=1048576
```

To limit wall-clock time (useful in CI):

```sh
cargo fuzz run fuzz_parse_epub -- -max_total_time=120 -max_len=1048576
```

## Reproducing a crash

When libFuzzer finds a crash it writes the minimised input to `fuzz/artifacts/<target>/`:

```sh
cargo fuzz run fuzz_parse_epub fuzz/artifacts/fuzz_parse_epub/crash-<hash>
```

`cargo fuzz tmin` can minimise the artifact further before filing a bug:

```sh
cargo fuzz tmin fuzz_parse_epub fuzz/artifacts/fuzz_parse_epub/crash-<hash>
```

## Adding seed corpus

Drop minimal valid files into the appropriate corpus directory before running:

```
fuzz/corpus/fuzz_parse_epub/   ← small valid .epub files
fuzz/corpus/fuzz_parse_docx/   ← minimal valid .docx files
fuzz/corpus/fuzz_parse_txt/    ← plain-text samples in various encodings
fuzz/corpus/fuzz_web_extract/  ← URL strings or HTML snippets
```

Seed files from the test fixtures are a good starting point:

```sh
cp fixtures/epub/*.epub fuzz/corpus/fuzz_parse_epub/
cp fixtures/docx/*.docx fuzz/corpus/fuzz_parse_docx/
```

## Notes

- All targets use restrictive `ParseLimits` (1 MB / 20 pages / depth 50 / 4 MB expanded)
  to keep individual fuzz runs fast.
- The `fuzz_web_extract` harness currently exercises URL-validation and early-exit
  paths only. Once `gist_web::extract_html` is made `pub` (post-M2), update the
  harness to pass raw HTML bytes directly.
- ePub and DOCX targets will start finding real bugs only after seed corpus files
  are present — libFuzzer is unlikely to generate valid ZIP magic spontaneously.
