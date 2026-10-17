# Linux GTK4 feasibility spike

This isolated GTK4 app initializes a real `gist-core` store on a worker thread
and provides an incremental first pass at the library and reading workflow:

- Browse up to 200 library items and search document text, titles, and source
  filenames with the Rust search path.
- Request TXT, ePub, DOCX, or PDF imports through GTK's native file chooser.
  TXT import has been manually verified. PDF import uses the pinned
  `libpdfium.so` from `./tools/fetch-pdfium.sh` (run it first; debug builds
  find it in `artifacts/pdfium/lib`). Without it, PDF import reports a typed
  "PDF support unavailable" error. GUI PDF import is not yet manually verified.
- Remove an item after confirmation; only GIST's internal copy and metadata
  are deleted, never the original selected file.
- Read document text in a basic Flow view or use RSVP playback backed by
  `gist-rsvp`, including pause/resume, a visible WPM readout and 100–1000 WPM
  slider (25 WPM steps), an interactive position scrubber with token count,
  back-five-words, and saved reading progress.

The current UI is a development spike, not a supported Linux release. It is
not yet feature-complete: collections, tags, sorting/filtering, URL import,
encryption/keyring integration, integrity UI, annotations, OCR, accessibility
verification, and packaging remain out of scope. Encryption uses a key held in the
desktop Secret Service (GNOME Keyring, KWallet, KeePassXC; ADR-031): the **Encrypt** button encrypts the
selected item's internal copy, and a banner explains key-store problems. With no key service,
unencrypted items still work and Encrypt reports it. The import-time copy under `originals/` is not
encrypted (ADR-014, `A5`).

## Display checks

`cargo run --example chooser_probe` needs a graphical session. It opens the real Import chooser and
checks that a dialog window appears, that a second open is ignored and that cancelling releases it.

## Prerequisites

- Rust toolchain pinned by the repository.
- GTK4 development files and `pkg-config` (Ubuntu/Debian: `sudo apt install
  libgtk-4-dev`).
- A running graphical session.

## Run

From the repository root:

```sh
cargo run --manifest-path spikes/linux-gtk/Cargo.toml
```

The app keeps its isolated library under
`$XDG_DATA_HOME/gist-linux-gtk-spike`, or `$HOME/.local/share/gist-linux-gtk-spike`
when `XDG_DATA_HOME` is unset. Do not point it at a production GIST library:
the spike does not yet provide the production app's encryption/keyring
integration or feature/security guarantees.

The spike is excluded from the root Cargo workspace and does not establish
the Linux product decisions, security, accessibility, or packaging gates in
`docs/linux-development-plan.md`.
