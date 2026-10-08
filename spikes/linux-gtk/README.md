# Linux GTK4 feasibility spike

This isolated GTK4 app initializes a real `gist-core` store on a worker thread
and provides an incremental first pass at the library and reading workflow:

- Browse up to 200 library items and search document text, titles, and source
  filenames with the Rust search path.
- Request TXT, ePub, DOCX, or PDF imports through GTK's native file chooser.
  TXT import has been manually verified. PDF import is currently unavailable
  because this spike has no Linux PDFium setup; it is not a supported format
  here.
- Remove an item after confirmation; only GIST's internal copy and metadata
  are deleted, never the original selected file.
- Read document text in a basic Flow view or use RSVP playback backed by
  `gist-rsvp`, including pause/resume, a visible WPM readout and 100–1000 WPM
  slider (25 WPM steps), an interactive position scrubber with token count,
  back-five-words, and saved reading progress.

The current UI is a development spike, not a supported Linux release. It is
not yet feature-complete: collections, tags, sorting/filtering, URL import,
encryption/keyring integration, integrity UI, annotations, OCR, accessibility
verification, and packaging remain out of scope. Encryption is not
implemented, and PDF import is unavailable without a Linux PDFium setup.

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
