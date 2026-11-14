#!/usr/bin/env python3
"""Copy the game data from the original Harry Potter 1 PC CD into gamedata/.

Only data packages, localisation text and ini files are copied. The Windows
executables, drivers, installers and copy-protection files stay on the disc:
the port does not use them.

Usage: python3 tools/install_from_cd.py [--cd /Volumes/HARRY_POTTER_EFG] [--lang eng|fre|ger]
"""
import argparse
import os
import shutil
import sys

DATA_DIRS = ["System", "Maps", "Textures", "Sounds", "Music", "save", "Help"]
LANG_DIRS = {"eng": "0", "fre": "1", "ger": "2"}

# Package, localisation and config files. Everything else is left behind.
KEEP_EXT = {
    "u", "unr", "utx", "uax", "umx", "int", "ini", "bmp", "wav", "ico",
    "eng", "fre", "ger",
    "eng_uax", "fre_uax", "ger_uax", "eng_utx", "fre_utx", "ger_utx", "hun_utx",
}


def wanted(name):
    ext = name.rsplit(".", 1)[-1].lower() if "." in name else ""
    return ext in KEEP_EXT


def copy_file(src, dst, stats):
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    if os.path.exists(dst) and os.path.getsize(dst) == os.path.getsize(src):
        stats["skipped"] += 1
        return
    shutil.copyfile(src, dst)
    os.chmod(dst, 0o644)
    stats["copied"] += 1
    stats["bytes"] += os.path.getsize(dst)


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--cd", default="/Volumes/HARRY_POTTER_EFG")
    ap.add_argument("--dest", default=os.path.join(here, "..", "gamedata"))
    ap.add_argument("--lang", choices=sorted(LANG_DIRS), default="eng")
    args = ap.parse_args()

    cd = args.cd
    dest = os.path.abspath(args.dest)
    if not os.path.isfile(os.path.join(cd, "System", "HPBase.u")):
        sys.exit(f"{cd} does not look like the Harry Potter 1 disc (System/HPBase.u missing)")

    stats = {"copied": 0, "skipped": 0, "bytes": 0}
    for d in DATA_DIRS:
        base = os.path.join(cd, d)
        for name in sorted(os.listdir(base)):
            src = os.path.join(base, name)
            if os.path.isfile(src):
                if wanted(name):
                    copy_file(src, os.path.join(dest, d, name), stats)
            elif name in LANG_DIRS.values():
                # Per-language folders are flattened into the parent, as the
                # original installer does. All three languages are installed;
                # file names are distinct per language except Default.ini.
                for sub in sorted(os.listdir(src)):
                    if sub.lower() == "default.ini" or not wanted(sub):
                        continue
                    copy_file(os.path.join(src, sub), os.path.join(dest, d, sub), stats)
            elif os.path.isdir(src):
                for root, _, files in os.walk(src):
                    for f in files:
                        if wanted(f):
                            rel = os.path.relpath(os.path.join(root, f), base)
                            copy_file(os.path.join(root, f), os.path.join(dest, d, rel), stats)

    # Default.ini only differs by its Language= line between the three folders.
    ini_src = os.path.join(cd, "System", LANG_DIRS[args.lang], "Default.ini")
    ini_dst = os.path.join(dest, "System", "Default.ini")
    shutil.copyfile(ini_src, ini_dst)
    os.chmod(ini_dst, 0o644)

    print(f"copied {stats['copied']} files ({stats['bytes'] / 1e6:.0f} MB), "
          f"{stats['skipped']} already present, language={args.lang}")
    print(f"game data is in {dest}")


if __name__ == "__main__":
    main()
