#!/usr/bin/env python3
"""Check that an APK or AAB supports 16 KB memory pages.

Google Play requires 16 KB page support for apps that target Android 15 or
later. For every 64-bit native library (arm64-v8a, x86_64) this checks:

* each ELF LOAD segment is aligned to 16 KB (linked with
  `-z max-page-size=16384`; NDK r28+ does this by default), and
* in an APK, a library stored uncompressed starts on a 16 KB boundary in the
  zip (`zipalign -P 16`), so it can be mapped straight from the APK.

32-bit libraries (armeabi-v7a, x86) are listed but not required: Android
devices with 16 KB pages are 64-bit only. In an AAB the zip offsets do not
matter, since Play builds the device APKs itself.

With --release it also checks the release packaging of native code: no
library ships `.debug_*` sections (AGP stripped it), and an AAB carries each
library's symbol table under BUNDLE-METADATA/com.android.tools.build.debugsymbols/
(`ndk.debugSymbolLevel = "SYMBOL_TABLE"`), which Play uses for crash reports.

Usage: check_android_alignment.py [--require-native] [--release] FILE.apk|FILE.aab ...
Exit status 1 if any library fails, or if --require-native finds no native
library at all (an armeabi-v7a-only APK has no 64-bit library to check).
"""
import argparse
import struct
import sys
import zipfile
from pathlib import Path, PurePosixPath

PAGE = 16 * 1024
ABIS_64 = {"arm64-v8a", "x86_64"}
ABIS_32 = {"armeabi-v7a", "x86"}
PT_LOAD = 1
SYMBOLS = "BUNDLE-METADATA/com.android.tools.build.debugsymbols"


def native_abi(name, bundle):
    """ABI directory of a native library entry, or None if it is not one."""
    parts = PurePosixPath(name).parts
    # APK: lib/<abi>/x.so; AAB: <module>/lib/<abi>/x.so
    prefix = 2 if bundle else 1
    if len(parts) != prefix + 2 or parts[prefix - 1] != "lib" or not parts[-1].endswith(".so"):
        return None
    return parts[prefix]


def load_alignments(data):
    """p_align of every LOAD segment of an ELF image."""
    if data[:4] != b"\x7fELF" or len(data) < 52:
        raise ValueError("not an ELF file")
    wide = {1: False, 2: True}.get(data[4])
    order = {1: "<", 2: ">"}.get(data[5])
    if wide is None or order is None:
        raise ValueError("unknown ELF class or byte order")
    if wide:
        phoff, = struct.unpack_from(order + "Q", data, 32)
        phentsize, phnum = struct.unpack_from(order + "HH", data, 54)
        layout, align_at = order + "I", 48
    else:
        phoff, = struct.unpack_from(order + "I", data, 28)
        phentsize, phnum = struct.unpack_from(order + "HH", data, 42)
        layout, align_at = order + "I", 28
    if phnum == 0 or phentsize < (56 if wide else 32) or phoff + phnum * phentsize > len(data):
        raise ValueError("truncated or missing program headers")
    aligns = []
    for index in range(phnum):
        entry = phoff + index * phentsize
        kind, = struct.unpack_from(layout, data, entry)
        if kind == PT_LOAD:
            aligns.append(struct.unpack_from(order + ("Q" if wide else "I"), data, entry + align_at)[0])
    if not aligns:
        raise ValueError("no LOAD segments")
    return aligns


def section_names(data):
    """Names of every section of an ELF image (empty when it has none)."""
    wide = data[4] == 2
    order = "<" if data[5] == 1 else ">"
    if wide:
        shoff, = struct.unpack_from(order + "Q", data, 40)
        shentsize, shnum, shstrndx = struct.unpack_from(order + "HHH", data, 58)
        offset_at, field = 24, "Q"
    else:
        shoff, = struct.unpack_from(order + "I", data, 32)
        shentsize, shnum, shstrndx = struct.unpack_from(order + "HHH", data, 46)
        offset_at, field = 16, "I"
    if shoff == 0 or shnum == 0:
        return []
    if shstrndx >= shnum or shoff + shnum * shentsize > len(data):
        raise ValueError("truncated section headers")
    def header(index):
        entry = shoff + index * shentsize
        name, = struct.unpack_from(order + "I", data, entry)
        offset, size = struct.unpack_from(order + field * 2, data, entry + offset_at)
        return name, offset, size
    _name, strings, strings_size = header(shstrndx)
    table = data[strings:strings + strings_size]
    names = []
    for index in range(shnum):
        name = header(index)[0]
        names.append(table[name:table.find(b"\0", name)].decode("ascii", "replace"))
    return names


def data_offset(archive, info):
    """Offset of an entry's data, from its local file header."""
    archive.fp.seek(info.header_offset)
    header = archive.fp.read(30)
    if header[:4] != b"PK\x03\x04":
        raise ValueError("bad local file header")
    name_length, extra_length = struct.unpack_from("<HH", header, 26)
    return info.header_offset + 30 + name_length + extra_length


def check(path, release=False):
    """(problems, notes, native library count) for one APK or AAB."""
    path = Path(path)
    bundle = path.suffix.lower() == ".aab"
    problems, notes, count = [], [], 0
    with zipfile.ZipFile(path) as archive:
        names = set(archive.namelist())
        for info in archive.infolist():
            abi = native_abi(info.filename, bundle)
            if abi is None:
                continue
            label = f"{path.name}:{info.filename}"
            count += 1
            try:
                data = archive.read(info)
                aligns = load_alignments(data)
                debug = [n for n in section_names(data) if n.startswith(".debug_")] if release else []
            except ValueError as error:
                problems.append(f"{label}: {error}")
                continue
            if debug:
                problems.append(f"{label}: not stripped, has {', '.join(debug[:3])}"
                                + (" ..." if len(debug) > 3 else "") + " (release libraries must be stripped)")
            library = PurePosixPath(info.filename).name
            if release and bundle and not ({f"{SYMBOLS}/{abi}/{library}.sym", f"{SYMBOLS}/{abi}/{library}.dbg"} & names):
                problems.append(f"{label}: no native debug symbols in {SYMBOLS}/{abi}/ "
                                '(ndk.debugSymbolLevel = "SYMBOL_TABLE")')
            low = min(aligns)
            if abi in ABIS_32:
                notes.append(f"{label}: 32-bit, LOAD alignment {low:#x} (not required)")
                continue
            if abi not in ABIS_64:
                problems.append(f"{label}: unknown ABI directory {abi!r}")
                continue
            if any(align % PAGE for align in aligns):
                problems.append(f"{label}: LOAD segment alignment {low:#x}, needs {PAGE:#x} "
                                "(link with -z max-page-size=16384 or NDK r28+)")
            if not bundle and info.compress_type == zipfile.ZIP_STORED:
                offset = data_offset(archive, info)
                if offset % PAGE:
                    problems.append(f"{label}: stored at zip offset {offset:#x}, not 16 KB aligned "
                                    "(zipalign -P 16)")
            notes.append(f"{label}: LOAD alignment {low:#x}"
                         + ("" if bundle else ", compressed" if info.compress_type != zipfile.ZIP_STORED
                            else ", stored 16 KB aligned"))
    return problems, notes, count


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--require-native", action="store_true",
                        help="fail when a package has no native library")
    parser.add_argument("--release", action="store_true",
                        help="also require stripped libraries and, in an AAB, native debug symbols")
    parser.add_argument("packages", nargs="+", type=Path)
    args = parser.parse_args(argv)
    failed = False
    for package in args.packages:
        problems, notes, count = check(package, release=args.release)
        for line in notes:
            print(line)
        if args.require_native and count == 0:
            problems.append(f"{package.name}: no native library found")
        for line in problems:
            print(f"error: {line}", file=sys.stderr)
        failed = failed or bool(problems)
        print(f"{package.name}: {count} native libraries, "
              + ("FAILED" if problems else "16 KB page compatible"
                 + (", stripped" + (", debug symbols included" if package.suffix.lower() == ".aab" else "")
                    if args.release else "")))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
