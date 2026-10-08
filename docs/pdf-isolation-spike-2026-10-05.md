# PDF isolation spike results (F33), 2026-10-05

Companion to `docs/adr/022-pdf-parser-process-isolation.md`. Code: `spikes/pdf-isolation/` (own `[workspace]`, listed in the root `exclude`, not in any shipping target, not referenced by `project.yml`).

**Caveat that applies to every number below:** measured on the repository's synthetic and hostile fixtures only (`fixtures/pdf/*`, `fixtures/pdf/adversarial/*`) plus one generated synthetic text-heavy PDF (400 pages, 1.4 MB, `gen_large_pdf.py`, kept in the scratchpad, not committed). No real-document corpus exists (decision D6). One machine (macOS 27, Apple Silicon), release builds, **unsigned**, no Hardened Runtime, no App Sandbox on the host. The design measured is the `posix_spawn`ed helper (option b). **An XPC service was not built or measured.**

## What was built

- `spike-helper`: reads a PDF from stdin, runs the real `gist_parse_pdf::parse_pdf` (default `ParseLimits`, real `libpdfium.dylib`), writes a framed reply: status byte, then the `Document` as JSON, or a typed error `kind<TAB>message`. Fault-injection markers in the input make it `abort()`, write through a null pointer, or hang, after pdfium is loaded.
- `spike-host`: `inproc` mode parses in the same process (baseline); `isolated` mode spawns the helper (input by pipe, or `--fd` handing the helper the file as stdin), reads the reply, deserialises the `Document`, and classifies a signal, non-zero exit, protocol error or 20 s timeout as a typed `HelperCrashed{...}` outcome. Peak RSS: host via `getrusage(RUSAGE_SELF)`, helper via `wait4` rusage. Cross-check: `/usr/bin/time -l` on the in-process large-PDF run reported 59,326,464 B; the host's own `getrusage` reported 59,260,928 B.

## Reproduce

```text
cd spikes/pdf-isolation
cargo build --release --offline                      # builds spike-helper and spike-host
python3 gen_large_pdf.py $SCRATCH/large.pdf 400 45
./bench.sh  <repo-root> $SCRATCH/large.pdf /tmp 5 > bench.tsv   # 5 runs x {inproc, pipe, fd} per file
./summarize.sh bench.tsv                             # median/min/max table
./crash.sh  <repo-root> <dir with crash_{ABORT,SEGV,HANG}.pdf>  # fixture + marker line, see below
./sandbox.sh <repo-root> <profile with @ROOT@ substituted>      # optional sandbox-exec experiment
```

The crash inputs are `fixtures/pdf/plain_text.pdf` with one appended line `%GIST_SPIKE_FAULT_ABORT`, `..._SEGV` or `..._HANG`. The pdfium dylib is expected at `<repo-root>/artifacts/pdfium/lib/libpdfium.dylib` (`tools/fetch-pdfium.sh`).

## 1. Time and peak memory (5 runs each, median; synthetic/hostile fixtures only)

`host_MiB` is the host process's peak RSS; `help_MiB` is the helper's. `inproc` has no helper, its host figure includes pdfium. `pipe` = input written through a pipe; `fd` = input file handed over as the helper's stdin. Full per-run lines were produced by `bench.sh`; min and max columns are kept so outliers stay visible.

```text
file                         mode         med_ms    min_ms    max_ms  host_MiB  help_MiB     json_B      in_B  outcome
plain_text.pdf               inproc         15.6      15.0     345.2      16.3       0.0      26571      4135  Ok
plain_text.pdf               pipe           20.0      19.7     219.8       5.9      16.1      26571      4135  Ok
plain_text.pdf               fd             19.8      19.7      20.4       5.9      16.3      26571      4135  Ok
two_column.pdf               inproc         15.1      14.8      16.1      16.3       0.0      17489      2903  Ok
two_column.pdf               pipe           20.1      19.8      20.9       5.9      16.3      17489      2903  Ok
two_column.pdf               fd             19.7      19.5      20.2       5.9      16.2      17489      2903  Ok
image_only.pdf               inproc          2.2       2.1       3.3      11.6       0.0          0       727  Err(this 
image_only.pdf               pipe            5.6       5.6       5.8       5.8      11.6         76       727  Err(no_te
image_only.pdf               fd              5.7       5.6       7.2       5.7      11.6         76       727  Err(no_te
encrypted_password.pdf       inproc          2.2       2.1       3.1      11.3       0.0          0      1038  Err(this 
encrypted_password.pdf       pipe            5.6       5.6       6.0       5.8      11.2         93      1038  Err(encry
encrypted_password.pdf       fd              5.6       5.5       6.4       5.7      11.2         93      1038  Err(encry
garbage_after_header.pdf     inproc          2.2       2.0       3.0      11.2       0.0          0      2057  Err(malfo
garbage_after_header.pdf     pipe            5.6       5.6       5.7       5.8      11.1         59      2057  Err(malfo
garbage_after_header.pdf     fd              5.5       5.5       5.7       5.7      11.1         59      2057  Err(malfo
huge_declared_count.pdf      inproc         14.6      14.4      14.8      14.7       0.0        425       700  Ok
huge_declared_count.pdf      pipe           18.9      18.7      20.0       5.8      14.8        425       700  Ok
huge_declared_count.pdf      fd             19.1      18.9      20.1       5.8      14.8        425       700  Ok
huge_string_object.pdf       inproc         41.6      40.0      44.0      30.0       0.0      65951      3642  Ok
huge_string_object.pdf       pipe           45.0      44.6      45.5       6.0      30.0      65951      3642  Ok
huge_string_object.pdf       fd             45.0      44.7      45.5       6.0      30.0      65951      3642  Ok
page_count_bomb.pdf          inproc          2.7       2.7       3.9      12.0       0.0          0    210148  Err(resou
page_count_bomb.pdf          pipe            6.8       6.6       7.1       6.0      12.0         71    210148  Err(resou
page_count_bomb.pdf          fd              6.6       6.5       6.8       5.7      12.1         71    210148  Err(resou
truncated.pdf                inproc          2.1       2.1       3.1      11.2       0.0          0      2067  Err(malfo
truncated.pdf                pipe            5.8       5.8       6.4       5.8      11.2         59      2067  Err(malfo
truncated.pdf                fd              5.6       5.6       5.7       5.7      11.2         59      2067  Err(malfo
large.pdf                    inproc        293.7     293.0     300.0      56.6       0.0   17415825   1427430  Ok
large.pdf                    pipe          372.1     363.6     500.9      39.0      58.4   17415825   1427430  Ok
large.pdf                    fd            380.8     372.6     397.8      41.1      56.8   17415825   1427430  Ok
```

Outliers, not discarded: the first in-process `plain_text.pdf` run took 345 ms and one pipe run 220 ms; both are cold-start (first touch of the dylib / page cache) and every other run of the same row was 15-20 ms. Medians are unaffected.

Reading the table (synthetic/hostile fixtures only):

- Small inputs: the helper adds a fixed **~4-5 ms** (process spawn plus loading pdfium into a fresh process) to a 2-15 ms parse. Rejections that cost ~2 ms in-process cost ~6 ms isolated.
- The 400-page text-heavy PDF: 294 ms in-process vs 372 ms (pipe) / 381 ms (fd), i.e. **+78-87 ms (~27-30 %)**. The difference is dominated by serialising and deserialising a **17.4 MB** `Document` JSON, not by moving the 1.4 MB input. Handing the file as an fd was not faster than a pipe at these sizes.
- Memory: the helper's peak (58 MiB) is about the in-process peak (57 MiB). The host's peak in isolated mode is 39 MiB for this input (the deserialised `Document` and the reply buffer; no pdfium working set), against 57 MiB in-process. Total system memory is higher while both exist. For inputs near the 64 MiB text budget the reply JSON would be on the order of 12x the text (tokens), so the 17.4 MB figure scales roughly linearly: a budget-sized document would send several hundred MB across the boundary. That is the real IPC risk and was **not measured** (no such fixture; building one would be a text-budget stress test, not a realistic document).
- The Document JSON is the payload. Input bytes crossing: 0 with `--fd`, `in_B` with a pipe. A 256 MiB input over a pipe was not measured.

## 2. Crash containment (synthetic fault injection)

The helper deliberately faults after loading pdfium. This proves the **isolation mechanism**; it does not demonstrate a real pdfium bug.

| Injected fault | Host outcome | Host exit | Wall |
|---|---|---|---|
| `abort()` | `HelperCrashed{signal=6}` | 0 | 4.1 ms |
| null write (SIGSEGV) | `HelperCrashed{signal=11}` | 0 | 3.7 ms |
| infinite loop | `HelperCrashed{timeout}` after the 20 s watchdog killed it | 0 | 20 019 ms |
| normal PDF right after the three crashes | `Ok`, 26 571 B JSON | 0 | 35.9 ms |

The host survived every fault, reported a typed value, and parsed a normal file afterwards. The in-process equivalent of the first two rows would have terminated the host (not run, by definition).

## 3. Sandbox profile on the helper (optional experiment)

`sandbox-exec -f` with a `(deny default)` profile (no network, no file writes except `/dev/null`):

| Profile | Result |
|---|---|
| `helper.sb`: reads only `/usr/lib`, `/System/Library`, dyld cache, `/dev/null`, `/dev/urandom`, the dylib dir and the helper dir | The helper **aborted at startup** (signal 6, ~0.7 MiB RSS, before pdfium); the host reported `HelperCrashed{signal=6}`. I did not find which rule was missing (helper stderr is discarded in the spike). Not a finding about pdfium. |
| `helper-readall.sb`: same, plus `(allow file-read*)` everywhere | pdfium loaded and parsed both fixtures to `Ok`, 26 571 and 17 489 B JSON, 35 ms and 28 ms (includes `sandbox-exec` launch). |

What this does and does not show: with writes, network and process-spawn-beyond-exec denied, the same parser still works, so a write-and-network-denied helper is plausible. It does **not** show a read-restricted profile works (the strict one failed on a missing rule), and I did not test that a write or a connect is actually blocked. `sandbox-exec` is deprecated and is not what XPC uses; it only approximates the effect. App Sandbox entitlements on an XPC service were not exercised.

## 4. What I could not verify

- Signed, Hardened-Runtime, notarised behaviour: no signing identity here. Whether a second signed binary (helper or XPC service) passes library validation, loads `libpdfium.dylib` from its own location, or notarises as a nested item is **unknown**.
- Anything XPC: launchd registration, per-service sandbox, connection latency, restart-after-crash. Not built.
- Real documents: none used (D6). Timing, memory and the Document-JSON size on real books may differ; the JSON-to-input ratio here (12x for the generated file) is specific to the generator.
- Inputs near the 256 MiB limit and Documents near the 64 MiB text budget.
- macOS 14 and 15 (this machine is macOS 27).
- Windows: no code written; the protocol is OS-neutral but nothing was run.
- Any real pdfium crash. Fault injection is synthetic.

## 5. What this means for the ADR recommendation

Containment works as designed and is cheap for the common case (~5 ms fixed). The cost that grows is the Document JSON transfer, about 30 % extra wall time on the large generated file. The isolation is only as strong as the helper's sandbox, which the spike could not establish. See the ADR for the recommendation.
