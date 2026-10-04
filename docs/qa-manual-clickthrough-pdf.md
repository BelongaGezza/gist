# Manual click-through — PDF import (M6 R1/R2)

For a person with a running GIST build (Debug is fine; use a build with
`libpdfium.dylib` embedded: `ls GIST.app/Contents/Frameworks`). Nothing below
has been run by an agent — Vision OCR on real scans, PDFKit render quality and
the UI itself are unverified until someone ticks these.

## 1. Text PDFs

- [ ] Import (Library → Import) a text PDF with a text layer (e.g. a
      journal article). The file picker lists `.pdf` files and accepts one.
- [ ] It appears in the library with a sensible title; opens in RSVP and in
      Flow view; text reads in the right order (two-column PDFs especially).
- [ ] Search finds a word from the PDF.

## 2. Password-protected PDF

- [ ] Import a PDF that needs a password to open. A "Password-Protected PDF"
      alert appears naming the file and saying GIST never bypasses
      protection; nothing is added to the library; no generic "Error" alert
      follows.

## 3. Scanned (image-only) PDF

- [ ] Import a scanned PDF (no selectable text). The OCR sheet opens by
      itself (no error alert) and shows "Preparing PDF pages — page N of M"
      with a progress bar, then "Recognizing text".
- [ ] Press Cancel during preparation: the sheet shows "Scan Cancelled", no
      item is added.
- [ ] Repeat and let it finish: the review screen lists every page, text is
      editable, low-confidence pages are flagged. Judge OCR accuracy and the
      page-image sharpness (rendered at 200 DPI) — note any pages that look
      blurry or cropped.
- [ ] Import: the item appears in the library and opens in RSVP/Flow.
- [ ] "Try Again" after a cancel re-runs the PDF (does not ask for images).
- [ ] While importing a large scan (50+ pages), Activity Monitor shows GIST's
      memory staying roughly flat (pages are rendered one at a time).
- [ ] After closing the sheet (success, cancel, or failure), no
      `gist-pdf-ocr-*` directory remains under `$TMPDIR` (the sandbox
      container's `tmp` — `ls "$(getconf DARWIN_USER_TEMP_DIR)"` won't show
      the container's; check
      `~/Library/Containers/com.gist.macos/Data/tmp`).

## 4. Limits and bad files

- [ ] A truncated/corrupt `.pdf` shows a clear failure, no crash.
- [ ] A PDF with more than 2,000 pages is refused up front with a page-count
      message (before any rendering starts).

## 5. Signed build (human/credential-gated)

- [ ] In a signed, notarised Release build, text-PDF import still works
      (embedded `libpdfium.dylib` passes library validation — see `N8`).
