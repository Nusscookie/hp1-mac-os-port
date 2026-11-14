#!/usr/bin/env python3
"""Inspect Unreal Engine 1 packages (.u .unr .utx .uax .umx).

  ue1pkg.py info    <package>...          header and export counts per class
  ue1pkg.py natives <package>...          native functions declared in a script package
  ue1pkg.py exports <package> [class]     list exports, optionally of one class
  ue1pkg.py scripts <package>... --out D  dump embedded UnrealScript source to D/<Package>/<Class>.uc
"""
import argparse
import collections
import os
import struct
import sys

FUNC_NET = 0x40
FUNC_NATIVE = 0x400


def compact_index(d, o):
    b = d[o]
    o += 1
    neg = b & 0x80
    v = b & 0x3F
    if b & 0x40:
        shift = 6
        while True:
            b = d[o]
            o += 1
            v |= (b & 0x7F) << shift
            shift += 7
            if not b & 0x80:
                break
    return (-v if neg else v), o


Export = collections.namedtuple("Export", "cls super outer name flags size offset")
Import = collections.namedtuple("Import", "cls_package cls_name outer name")


class Package:
    def __init__(self, path):
        self.path = path
        self.name = os.path.splitext(os.path.basename(path))[0]
        with open(path, "rb") as f:
            self.data = d = f.read()
        (tag, self.version, self.licensee, self.flags,
         nc, no, ec, eo, ic, io) = struct.unpack("<IHHIIIIIII", d[:36])
        if tag != 0x9E2A83C1:
            raise ValueError(f"{path}: not an Unreal package")

        self.names = []
        o = no
        for _ in range(nc):
            if self.version >= 64:
                n, o = compact_index(d, o)
                s = d[o:o + n - 1]
                o += n
            else:
                e = d.index(b"\0", o)
                s = d[o:e]
                o = e + 1
            o += 4  # name flags
            self.names.append(s.decode("latin1"))

        self.imports = []
        o = io
        for _ in range(ic):
            cp, o = compact_index(d, o)
            cn, o = compact_index(d, o)
            outer = struct.unpack("<i", d[o:o + 4])[0]
            o += 4
            on, o = compact_index(d, o)
            self.imports.append(Import(self.names[cp], self.names[cn], outer, self.names[on]))

        self.exports = []
        o = eo
        for _ in range(ec):
            c, o = compact_index(d, o)
            s, o = compact_index(d, o)
            outer = struct.unpack("<i", d[o:o + 4])[0]
            o += 4
            n, o = compact_index(d, o)
            fl = struct.unpack("<I", d[o:o + 4])[0]
            o += 4
            size, o = compact_index(d, o)
            off = 0
            if size > 0:
                off, o = compact_index(d, o)
            self.exports.append(Export(c, s, outer, self.names[n], fl, size, off))

    def ref_name(self, ref):
        if ref == 0:
            return None
        return self.imports[-ref - 1].name if ref < 0 else self.exports[ref - 1].name

    def class_of(self, e):
        return self.ref_name(e.cls) or "Class"

    def outer_of(self, e):
        return self.ref_name(e.outer)

    def blob(self, e):
        return self.data[e.offset:e.offset + e.size]

    def natives(self):
        """(class, function, native index) for every native function. Index 0 = bound by name."""
        out = []
        for e in self.exports:
            if self.class_of(e) != "Function" or e.size < 9:
                continue
            b = self.blob(e)
            # UFunction tail: iNative u16, OperatorPrecedence u8, FunctionFlags u32, [RepOffset u16 if FUNC_Net]
            flags = struct.unpack("<I", b[-4:])[0]
            inative = struct.unpack("<H", b[-7:-5])[0]
            if flags & FUNC_NET or flags > 0xFFFFF:
                flags = struct.unpack("<I", b[-6:-2])[0]
                inative = struct.unpack("<H", b[-9:-7])[0]
            if flags & FUNC_NATIVE:
                out.append((self.outer_of(e), e.name, inative))
        return sorted(out)

    def scripts(self):
        """(class name, source text) for every embedded ScriptText buffer."""
        for e in self.exports:
            if self.class_of(e) != "TextBuffer" or e.name != "ScriptText" or e.size == 0:
                continue
            b = self.blob(e)
            o = 0
            prop, o = compact_index(b, o)
            if self.names[prop] != "None":
                continue
            o += 8  # Pos, Top
            n, o = compact_index(b, o)
            yield self.outer_of(e), b[o:o + max(n - 1, 0)].decode("latin1")


def cmd_info(args):
    for p in args.packages:
        pkg = Package(p)
        kinds = collections.Counter(pkg.class_of(e) for e in pkg.exports)
        print(f"{pkg.name}: version={pkg.version} licensee={pkg.licensee} flags={pkg.flags:#x} "
              f"names={len(pkg.names)} imports={len(pkg.imports)} exports={len(pkg.exports)}")
        for k, v in kinds.most_common():
            print(f"  {v:6d} {k}")


def cmd_natives(args):
    for p in args.packages:
        pkg = Package(p)
        by_class = collections.defaultdict(list)
        for cls, fn, idx in pkg.natives():
            by_class[cls].append(f"{fn}#{idx}" if idx else fn)
        print(f"{pkg.name}: {sum(map(len, by_class.values()))} natives")
        for cls in sorted(by_class):
            print(f"  {cls}: {', '.join(by_class[cls])}")


def cmd_exports(args):
    pkg = Package(args.package)
    for i, e in enumerate(pkg.exports):
        cls = pkg.class_of(e)
        if args.cls and cls.lower() != args.cls.lower():
            continue
        print(f"{i + 1:6d} {cls:16s} {pkg.outer_of(e) or '':24s} {e.name:40s} size={e.size} offset={e.offset}")


def cmd_scripts(args):
    total = 0
    for p in args.packages:
        pkg = Package(p)
        outdir = os.path.join(args.out, pkg.name)
        for cls, text in pkg.scripts():
            os.makedirs(outdir, exist_ok=True)
            with open(os.path.join(outdir, f"{cls}.uc"), "w", encoding="utf-8", newline="") as f:
                f.write(text.replace("\r\n", "\n"))
            total += 1
    print(f"wrote {total} script files to {args.out}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name, fn in (("info", cmd_info), ("natives", cmd_natives)):
        sp = sub.add_parser(name)
        sp.add_argument("packages", nargs="+")
        sp.set_defaults(fn=fn)
    sp = sub.add_parser("exports")
    sp.add_argument("package")
    sp.add_argument("cls", nargs="?")
    sp.set_defaults(fn=cmd_exports)
    sp = sub.add_parser("scripts")
    sp.add_argument("packages", nargs="+")
    sp.add_argument("--out", required=True)
    sp.set_defaults(fn=cmd_scripts)
    args = ap.parse_args()
    args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
