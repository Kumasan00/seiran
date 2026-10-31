#!/usr/bin/env python3
"""tests/image/*.pdf（PDF 図版のテスト用 fixture）を再生成する。

標準ライブラリだけで書き出し、暗号化した 2 件だけ Ghostscript（gs）で作る。生成物はコミット済みで、
テストはこのスクリプトを実行しない（fixture を変えるときだけ手で実行する）。

    python3 tools/make-pdf-fixtures.py
"""

import struct
import subprocess
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "tests" / "image"

# ベクタの塗り矩形 2 つ（ラスタを含まない）
CONTENT = b"0 0.4 0.8 rg 20 20 160 80 re f 0.9 0.3 0.1 RG 4 w 30 30 m 170 90 l S"


def write_pdf(name, objects, header="1.7"):
    """1 始まりのオブジェクト本体の列から、正しい xref 付きの PDF を書く（1 番が Catalog）。"""
    out = bytearray(f"%PDF-{header}\n%\xe2\xe3\xcf\xd3\n".encode("latin-1"))
    offsets = []
    for number, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{number} 0 obj\n".encode() + body + b"\nendobj\n"
    xref = len(out)
    out += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode()
    for offset in offsets:
        out += f"{offset:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    (OUT / name).write_bytes(bytes(out))


def tiny_png():
    """1x1 の PNG（拡張子 .pdf の中身が別形式のときの fixture）。"""

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(b"\x00\xff\x00\x00")) + chunk(b"IEND", b"")


def stream(data):
    return f"<< /Length {len(data)} >>\nstream\n".encode() + data + b"\nendstream"


def pages_doc(name, page_dicts, header="1.7", catalog_extra=b""):
    """各ページ辞書（/Type /Page と /Parent・/Contents を除く中身）からページ列の PDF を書く。"""
    count = len(page_dicts)
    kids = " ".join(f"{3 + i * 2} 0 R" for i in range(count))
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R " + catalog_extra + b">>",
        f"<< /Type /Pages /Kids [{kids}] /Count {count} >>".encode(),
    ]
    for i, extra in enumerate(page_dicts):
        content_ref = 4 + i * 2
        objects.append(
            f"<< /Type /Page /Parent 2 0 R /Contents {content_ref} 0 R ".encode() + extra + b" >>"
        )
        objects.append(stream(CONTENT))
    write_pdf(name, objects, header)


def main():
    one_page = [b"/MediaBox [0 0 200 120]"]
    pages_doc("vector1.pdf", one_page)
    pages_doc("vector1_pdf14.pdf", one_page, header="1.4")
    pages_doc(
        "vector2.pdf",
        [
            b"/MediaBox [0 0 300 150]",
            b"/MediaBox [0 0 200 100] /CropBox [-50 10 110 160] /Rotate 90",
        ],
    )
    pages_doc(
        "annotated.pdf",
        [
            b"/MediaBox [0 0 200 120] /Annots [<< /Type /Annot /Subtype /Link /Rect [0 0 50 50] "
            b"/A << /S /URI /URI (https://example.com) >> >>]"
        ],
    )
    pages_doc("zero_area.pdf", [b"/MediaBox [0 0 200 100] /CropBox [0 0 200 0]"])
    write_pdf(
        "no_pages.pdf",
        [b"<< /Type /Catalog /Pages 2 0 R >>", b"<< /Type /Pages /Kids [] /Count 0 >>"],
    )
    pages_doc("pdf20_header.pdf", one_page, header="2.0")
    pages_doc("pdf20_catalog.pdf", one_page, catalog_extra=b"/Version /2.0 ")
    (OUT / "png_bytes.pdf").write_bytes(tiny_png())

    gs = ["gs", "-q", "-sDEVICE=pdfwrite", "-dCompatibilityLevel=1.7", "-dNOPAUSE", "-dBATCH"]
    source = str(OUT / "vector1.pdf")
    subprocess.run(
        gs + ["-sOwnerPassword=owner", "-sUserPassword=user", "-o", str(OUT / "encrypted_user.pdf"), source],
        check=True,
    )
    subprocess.run(
        gs + ["-sOwnerPassword=owner", "-o", str(OUT / "encrypted_owner_only.pdf"), source],
        check=True,
    )


if __name__ == "__main__":
    main()
