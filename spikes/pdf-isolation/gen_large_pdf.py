#!/usr/bin/env python3
"""Generate a large, text-heavy synthetic PDF (stdlib only) for the spike.

usage: gen_large_pdf.py OUT.pdf [pages=400] [lines_per_page=45]
Output goes to the scratchpad; never commit it (decision D6: no real PDFs,
no large generated files in git).
"""
import sys

out = sys.argv[1]
pages = int(sys.argv[2]) if len(sys.argv) > 2 else 400
lines = int(sys.argv[3]) if len(sys.argv) > 3 else 45

words = ("the quick brown fox jumps over a lazy dog while reading pages of "
         "synthetic prose about isolation sandboxes and parsers").split()
objs = []


def add(b):
    objs.append(b)
    return len(objs)


add(b"")  # 1 catalog placeholder
add(b"")  # 2 pages placeholder
font = add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>")
kids = []
for p in range(pages):
    ops = ["BT", "/F1 11 Tf", "14 TL", "56 740 Td"]
    for l in range(lines):
        txt = " ".join(words[(p + l + i) % len(words)] for i in range(11))
        ops.append("(%s) Tj T*" % txt)
    ops.append("ET")
    stream = "\n".join(ops).encode()
    c = add(b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream")
    pg = add(("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
              "/Contents %d 0 R /Resources << /Font << /F1 %d 0 R >> >> >>"
              % (c, font)).encode())
    kids.append(pg)
objs[0] = b"<< /Type /Catalog /Pages 2 0 R >>"
objs[1] = ("<< /Type /Pages /Count %d /Kids [%s] >>" % (
    pages, " ".join("%d 0 R" % k for k in kids))).encode()

buf = bytearray(b"%PDF-1.4\n")
offs = []
for i, o in enumerate(objs, 1):
    offs.append(len(buf))
    buf += b"%d 0 obj\n" % i + o + b"\nendobj\n"
xref = len(buf)
buf += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
for o in offs:
    buf += b"%010d 00000 n \n" % o
buf += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, xref)
open(out, "wb").write(buf)
print(out, len(buf), "bytes")
