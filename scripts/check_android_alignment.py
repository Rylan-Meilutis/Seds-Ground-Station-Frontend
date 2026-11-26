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

Usage: check_android_alignment.py [--require-native] FILE.apk|FILE.aab ...
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


def data_offset(archive, info):
    """Offset of an entry's data, from its local file header."""
    archive.fp.seek(info.header_offset)
    header = archive.fp.read(30)
    if header[:4] != b"PK\x03\x04":
        raise ValueError("bad local file header")
    name_length, extra_length = struct.unpack_from("<HH", header, 26)
    return info.header_offset + 30 + name_length + extra_length


def check(path):
    """(problems, notes, native library count) for one APK or AAB."""
    path = Path(path)
    bundle = path.suffix.lower() == ".aab"
    problems, notes, count = [], [], 0
    with zipfile.ZipFile(path) as archive:
        for info in archive.infolist():
            abi = native_abi(info.filename, bundle)
            if abi is None:
                continue
            label = f"{path.name}:{info.filename}"
            count += 1
            try:
                aligns = load_alignments(archive.read(info))
            except ValueError as error:
                problems.append(f"{label}: {error}")
                continue
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
    parser.add_argument("packages", nargs="+", type=Path)
    args = parser.parse_args(argv)
    failed = False
    for package in args.packages:
        problems, notes, count = check(package)
        for line in notes:
            print(line)
        if args.require_native and count == 0:
            problems.append(f"{package.name}: no native library found")
        for line in problems:
            print(f"error: {line}", file=sys.stderr)
        failed = failed or bool(problems)
        print(f"{package.name}: {count} native libraries, "
              + ("NOT 16 KB page compatible" if problems else "16 KB page compatible"))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
