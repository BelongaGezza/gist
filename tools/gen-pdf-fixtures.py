#!/usr/bin/env python3
"""Generate the synthetic PDF fixtures under fixtures/pdf/ (M6 R1).

Dependency-free (stdlib only), deterministic, fully synthetic - no third-party
content. Re-run from the repo root:  python3 tools/gen-pdf-fixtures.py
Not used by any hook or CI step (so the Windows python3 Store stub is moot).
"""
import hashlib
import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "fixtures", "pdf")
ADV = os.path.join(OUT, "adversarial")

PAD = bytes.fromhex(
    "28BF4E5E4E758A4164004E56FFFA01082E2E00B6D0683E802F0CA9FE6453697A"
)


def rc4(key: bytes, data: bytes) -> bytes:
    s = list(range(256))
    j = 0
    for i in range(256):
        j = (j + s[i] + key[i % len(key)]) % 256
        s[i], s[j] = s[j], s[i]
    i = j = 0
    out = bytearray()
    for b in data:
        i = (i + 1) % 256
        j = (j + s[i]) % 256
        s[i], s[j] = s[j], s[i]
        out.append(b ^ s[(s[i] + s[j]) % 256])
    return bytes(out)


class Pdf:
    def __init__(self):
        self.objs = {}  # num -> bytes (body without "n 0 obj"/"endobj"), or ("stream", dict_str, data)
        self.next = 1

    def alloc(self):
        n = self.next
        self.next += 1
        return n

    def set(self, n, body):
        self.objs[n] = body

    def serialize(self, root, encrypt=None, info=None):
        out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
        offsets = {}
        for n in sorted(self.objs):
            offsets[n] = len(out)
            body = self.objs[n]
            out += f"{n} 0 obj\n".encode()
            if isinstance(body, tuple):
                _, d, data = body
                if encrypt:
                    data = encrypt(n, data)
                out += f"<< {d} /Length {len(data)} >>\nstream\n".encode()
                out += data + b"\nendstream"
            else:
                out += body if isinstance(body, bytes) else body.encode()
            out += b"\nendobj\n"
        xref = len(out)
        size = max(self.objs) + 1
        out += f"xref\n0 {size}\n".encode()
        out += b"0000000000 65535 f \n"
        for n in range(1, size):
            if n in offsets:
                out += f"{offsets[n]:010d} 00000 n \n".encode()
            else:
                out += b"0000000000 65535 f \n"
        trailer = f"/Size {size} /Root {root} 0 R"
        if info:
            trailer += f" /Info {info} 0 R"
        if encrypt:
            trailer += encrypt.trailer_extra
        out += f"trailer\n<< {trailer} >>\nstartxref\n{xref}\n%%EOF\n".encode()
        return bytes(out)


def esc(s):
    return s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


def text_ops(runs):
    """runs: list of (font_key, size, x, y, text)"""
    ops = []
    for font, size, x, y, text in runs:
        ops.append(f"BT /{font} {size} Tf {x} {y} Td ({esc(text)}) Tj ET")
    return "\n".join(ops).encode()


def build(pages, info_title=None, extra_page_res=""):
    """pages: list of content-stream bytes. Returns (Pdf, root, info)."""
    pdf = Pdf()
    cat = pdf.alloc()
    pages_n = pdf.alloc()
    f1 = pdf.alloc()
    f2 = pdf.alloc()
    pdf.set(f1, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    pdf.set(f2, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>")
    kids = []
    for content in pages:
        c = pdf.alloc()
        p = pdf.alloc()
        pdf.set(c, ("stream", "", content))
        pdf.set(
            p,
            f"<< /Type /Page /Parent {pages_n} 0 R /MediaBox [0 0 612 792] "
            f"/Resources << /Font << /F1 {f1} 0 R /F2 {f2} 0 R >> {extra_page_res} >> "
            f"/Contents {c} 0 R >>",
        )
        kids.append(p)
    pdf.set(
        pages_n,
        f"<< /Type /Pages /Kids [{' '.join(f'{k} 0 R' for k in kids)}] /Count {len(kids)} >>",
    )
    pdf.set(cat, f"<< /Type /Catalog /Pages {pages_n} 0 R >>")
    info = None
    if info_title:
        info = pdf.alloc()
        pdf.set(info, f"<< /Title ({esc(info_title)}) /Author (GIST Fixtures) >>")
    return pdf, cat, info


def wrap(words_per_line, text):
    ws = text.split()
    lines, cur = [], []
    for w in ws:
        cur.append(w)
        if len(cur) == words_per_line:
            lines.append(" ".join(cur))
            cur = []
    if cur:
        lines.append(" ".join(cur))
    return lines


BODY = (
    "The reading of a long document is mostly a matter of rhythm. A steady pace "
    "lets the eye settle while the mind assembles meaning from one word to the "
    "next, and small interruptions such as running headers or page numbers only "
    "get in the way of that rhythm."
)


def plain_text():
    pages = []
    for n in range(1, 4):
        runs = [("F1", 9, 72, 760, "GIST Synthetic Report")]
        y = 700
        if n == 1:
            runs.append(("F2", 24, 72, y, "A Short Synthetic Report"))
            y -= 50
        runs.append(("F2", 16, 72, y, f"Section {n}"))
        y -= 30
        for para in range(2):
            for line in wrap(11, BODY):
                runs.append(("F1", 11, 72, y, line))
                y -= 14
            y -= 10
        runs.append(("F1", 9, 300, 30, str(n)))
        pages.append(text_ops(runs))
    pdf, root, info = build(pages, "A Short Synthetic Report")
    return pdf.serialize(root, info=info)


def two_column():
    runs = [("F2", 20, 72, 720, "Two Column Layout")]
    left = wrap(6, BODY + " " + BODY)
    right = wrap(6, "Right column text begins here. " + BODY + " " + BODY)
    y = 680
    for i in range(max(len(left), len(right))):
        if i < len(left):
            runs.append(("F1", 11, 72, y, left[i]))
        if i < len(right):
            runs.append(("F1", 11, 330, y, right[i]))
        y -= 14
    pdf, root, info = build([text_ops(runs)], "Two Column Layout")
    return pdf.serialize(root, info=info)


def image_only():
    # A page whose only content is a 8x8 gray image XObject: no text layer.
    pdf = Pdf()
    cat, pages_n, img, c, p = (pdf.alloc() for _ in range(5))
    raw = bytes((x * 32) % 256 for x in range(64))
    pdf.set(img, ("stream", "/Type /XObject /Subtype /Image /Width 8 /Height 8 "
                  "/ColorSpace /DeviceGray /BitsPerComponent 8", raw))
    pdf.set(c, ("stream", "", b"q 400 0 0 400 100 200 cm /Im0 Do Q"))
    pdf.set(p, f"<< /Type /Page /Parent {pages_n} 0 R /MediaBox [0 0 612 792] "
               f"/Resources << /XObject << /Im0 {img} 0 R >> >> /Contents {c} 0 R >>")
    pdf.set(pages_n, f"<< /Type /Pages /Kids [{p} 0 R] /Count 1 >>")
    pdf.set(cat, f"<< /Type /Catalog /Pages {pages_n} 0 R >>")
    return pdf.serialize(cat)


def encrypted():
    user_pw = b"secret"
    owner_pw = b"owner-secret"
    file_id = hashlib.md5(b"gist-fixture").digest()
    p_val = -4
    # Algorithm 3: O entry (R2)
    okey = hashlib.md5((owner_pw + PAD)[:32]).digest()[:5]
    o = rc4(okey, (user_pw + PAD)[:32])
    # Algorithm 2: file key (R2)
    key = hashlib.md5(
        (user_pw + PAD)[:32] + o + struct.pack("<i", p_val) + file_id
    ).digest()[:5]
    u = rc4(key, PAD)

    def enc(n, data):
        k = hashlib.md5(key + struct.pack("<I", n)[:3] + b"\x00\x00").digest()[:10]
        return rc4(k, data)

    enc.trailer_extra = (
        f" /Encrypt {{E}} 0 R /ID [<{file_id.hex()}> <{file_id.hex()}>]"
    )
    runs = [("F1", 12, 72, 700, "This text is encrypted with a user password.")]
    pdf, root, info = build([text_ops(runs)])
    e = pdf.alloc()
    pdf.set(
        e,
        f"<< /Filter /Standard /V 1 /R 2 /O <{o.hex()}> /U <{u.hex()}> /P {p_val} >>",
    )
    enc.trailer_extra = enc.trailer_extra.replace("{E}", str(e))
    return pdf.serialize(root, encrypt=enc)


def page_bomb(n=2100):
    pdf = Pdf()
    cat = pdf.alloc()
    pages_n = pdf.alloc()
    kids = []
    for _ in range(n):
        p = pdf.alloc()
        pdf.set(p, f"<< /Type /Page /Parent {pages_n} 0 R /MediaBox [0 0 10 10] >>")
        kids.append(p)
    pdf.set(pages_n, f"<< /Type /Pages /Kids [{' '.join(f'{k} 0 R' for k in kids)}] /Count {n} >>")
    pdf.set(cat, f"<< /Type /Catalog /Pages {pages_n} 0 R >>")
    return pdf.serialize(cat)


def huge_count():
    # Claims an absurd /Count (and one real page) in a tiny file.
    pdf, root, _ = build([text_ops([("F1", 12, 72, 700, "tiny")])])
    data = pdf.serialize(root)
    return data.replace(b"/Count 1 ", b"/Count 999999999 ")


def main():
    os.makedirs(ADV, exist_ok=True)
    files = {
        os.path.join(OUT, "plain_text.pdf"): plain_text(),
        os.path.join(OUT, "two_column.pdf"): two_column(),
        os.path.join(OUT, "image_only.pdf"): image_only(),
        os.path.join(ADV, "encrypted_password.pdf"): encrypted(),
        os.path.join(ADV, "page_count_bomb.pdf"): page_bomb(),
        os.path.join(ADV, "huge_declared_count.pdf"): huge_count(),
    }
    pt = files[os.path.join(OUT, "plain_text.pdf")]
    files[os.path.join(ADV, "truncated.pdf")] = pt[: len(pt) // 2]
    files[os.path.join(ADV, "garbage_after_header.pdf")] = b"%PDF-1.7\n" + bytes(range(256)) * 8
    for path, data in files.items():
        with open(path, "wb") as f:
            f.write(data)
        print(f"{os.path.relpath(path, OUT)}: {len(data)} bytes")


if __name__ == "__main__":
    main()
