"""16 KB page-size checks for Android packages (Google Play, Android 15+)."""
import importlib.util
import io
import os
import struct
import sys
import tempfile
import unittest
import zipfile
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).parents[1]
spec = importlib.util.spec_from_file_location("check_android_alignment", ROOT / "scripts/check_android_alignment.py")
align = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = align
spec.loader.exec_module(align)


def elf(p_align, wide=True, loads=2):
    """ELF header + program headers: a PT_PHDR and `loads` PT_LOAD entries."""
    if wide:
        phoff, phentsize = 64, 56
        header = b"\x7fELF" + bytes([2, 1, 1]) + bytes(9)
        header += struct.pack("<HHIQQQIHHHHHH", 3, 183, 1, 0, phoff, 0, 0, 64, phentsize, loads + 1, 64, 0, 0)
        entry = lambda kind, a: struct.pack("<IIQQQQQQ", kind, 5, 0, 0, 0, 0x100, 0x100, a)
    else:
        phoff, phentsize = 52, 32
        header = b"\x7fELF" + bytes([1, 1, 1]) + bytes(9)
        header += struct.pack("<HHIIIIIHHHHHH", 3, 40, 1, 0, phoff, 0, 0, 52, phentsize, loads + 1, 40, 0, 0)
        entry = lambda kind, a: struct.pack("<IIIIIIII", kind, 0, 0, 0, 0x100, 0x100, 5, a)
    return header + entry(6, 8) + b"".join(entry(1, p_align) for _ in range(loads))


def package(path, entries):
    """entries: (name, data, stored, data_alignment or None)."""
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("AndroidManifest.xml", b"<manifest/>" * 7)
        for name, data, stored, boundary in entries:
            info = zipfile.ZipInfo(name)
            info.compress_type = zipfile.ZIP_STORED if stored else zipfile.ZIP_DEFLATED
            if boundary:
                start = archive.fp.tell() + 30 + len(name.encode())
                pad = (-start - 4) % boundary
                info.extra = struct.pack("<HH", 0xD935, pad) + bytes(pad)
            archive.writestr(info, data)
    return path


class AndroidAlignmentTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def run_main(self, *argv):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = align.main([str(a) for a in argv])
        return code, out.getvalue(), err.getvalue()

    def test_elf_load_alignments_32_and_64_bit(self):
        self.assertEqual(align.load_alignments(elf(0x4000)), [0x4000, 0x4000])
        self.assertEqual(align.load_alignments(elf(0x1000, wide=False)), [0x1000, 0x1000])
        for bad in [b"not an elf", elf(0x4000)[:70], elf(0x4000, loads=0)]:
            with self.subTest(bad=bad[:8]), self.assertRaises(ValueError):
                align.load_alignments(bad)

    def test_aligned_apk_passes(self):
        apk = package(self.root / "ok.apk", [
            ("lib/arm64-v8a/libmain.so", elf(0x4000), True, 0x4000),
            ("lib/x86_64/libc++_shared.so", elf(0x4000), False, None),
            ("lib/armeabi-v7a/libmain.so", elf(0x1000, wide=False), True, 0x1000),
        ])
        problems, notes, count = align.check(apk)
        self.assertEqual((problems, count), ([], 3))
        self.assertTrue(any("32-bit" in n and "not required" in n for n in notes))
        code, out, _ = self.run_main("--require-native", apk)
        self.assertEqual(code, 0)
        self.assertIn("16 KB page compatible", out)

    def test_4k_elf_segments_fail(self):
        apk = package(self.root / "elf.apk", [("lib/arm64-v8a/libmain.so", elf(0x1000), True, 0x4000)])
        problems, _, _ = align.check(apk)
        self.assertEqual(len(problems), 1)
        self.assertIn("max-page-size=16384", problems[0])
        code, out, err = self.run_main(apk)
        self.assertEqual(code, 1)
        self.assertIn("NOT 16 KB page compatible", out)
        self.assertIn("error:", err)

    def test_stored_library_off_a_16k_boundary_fails_in_apk_only(self):
        entries = [("lib/arm64-v8a/libmain.so", elf(0x4000), True, 0x1000)]
        apk = package(self.root / "zip.apk", entries)
        with zipfile.ZipFile(apk) as archive:
            info = archive.getinfo(entries[0][0])
            self.assertNotEqual(align.data_offset(archive, info) % 0x4000, 0, "fixture must be misaligned")
        problems, _, _ = align.check(apk)
        self.assertEqual(len(problems), 1)
        self.assertIn("zipalign -P 16", problems[0])
        # Play builds the device APKs from an AAB, so only the ELF matters there.
        aab = package(self.root / "zip.aab", [("base/lib/arm64-v8a/libmain.so", elf(0x4000), True, 0x1000)])
        self.assertEqual(align.check(aab)[0], [])
        bad = package(self.root / "bad.aab", [("base/lib/x86_64/libmain.so", elf(0x1000), False, None)])
        self.assertEqual(len(align.check(bad)[0]), 1)

    def test_armv7_only_apk_counts_as_native(self):
        apk = package(self.root / "armv7.apk", [("lib/armeabi-v7a/libmain.so", elf(0x1000, wide=False), True, 0x1000)])
        self.assertEqual(align.check(apk)[0::2], ([], 1))
        self.assertEqual(self.run_main("--require-native", apk)[0], 0)

    def test_require_native_and_entry_paths(self):
        empty = package(self.root / "wear.apk", [("assets/x.so", b"data", True, None)])
        self.assertEqual(align.check(empty), ([], [], 0))
        self.assertEqual(self.run_main(empty)[0], 0)
        self.assertEqual(self.run_main("--require-native", empty)[0], 1)
        self.assertEqual(align.native_abi("lib/arm64-v8a/a.so", False), "arm64-v8a")
        self.assertEqual(align.native_abi("base/lib/x86_64/a.so", True), "x86_64")
        self.assertIsNone(align.native_abi("base/lib/x86_64/a.so", False))
        self.assertIsNone(align.native_abi("lib/arm64-v8a/a.txt", False))
        odd = package(self.root / "odd.apk", [("lib/mips64/libmain.so", elf(0x4000), False, None)])
        self.assertIn("unknown ABI", align.check(odd)[0][0])

    def test_ci_driver_fails_the_step_on_misalignment(self):
        ci_spec = importlib.util.spec_from_file_location("frontend_ci_alignment", ROOT / "scripts/ci/frontend.py")
        ci = importlib.util.module_from_spec(ci_spec)
        ci_spec.loader.exec_module(ci)
        good = package(self.root / "good.aab", [("base/lib/arm64-v8a/libmain.so", elf(0x4000), False, None)])
        bad = package(self.root / "bad.apk", [("lib/arm64-v8a/libmain.so", elf(0x1000), True, 0x4000)])
        wear = package(self.root / "wear.apk", [])
        with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            ci.android_page_alignment([good], require_native=True)
            ci.android_page_alignment([wear], require_native=False)
            for packages, native in [([bad], True), ([good, bad], True), ([wear], True), ([], False)]:
                with self.subTest(packages=[p.name for p in packages]), self.assertRaises(RuntimeError):
                    ci.android_page_alignment(packages, require_native=native)

    def test_build_links_android_libraries_for_16k_pages(self):
        build_spec = importlib.util.spec_from_file_location("build_alignment", ROOT / "build.py")
        build = importlib.util.module_from_spec(build_spec)
        sys.modules[build_spec.name] = build
        build_spec.loader.exec_module(build)
        self.assertEqual(build.ANDROID_PAGE_SIZE_LINK_ARG, "link-arg=-Wl,-z,max-page-size=16384")
        flag = "-C " + build.ANDROID_PAGE_SIZE_LINK_ARG
        with patch.object(build, "_detect_android_sdk_root", return_value=None), \
                redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            env = build._ensure_android_env(self.root, {"RUSTFLAGS": "-D warnings", "PATH": "/usr/bin"})
            self.assertEqual(env["RUSTFLAGS"], "-D warnings " + flag)
            again = build._ensure_android_env(self.root, env)
        self.assertEqual(again["RUSTFLAGS"].count(build.ANDROID_PAGE_SIZE_LINK_ARG), 1, "idempotent")
        # Play needs target API 36; the template's AGP 8.7.0 warns above 35 (fatal in CI).
        props = self.root / "gradle.properties"
        props.write_text("android.useAndroidX=true\nandroid.defaults.buildfeatures.buildconfig=true\n")
        with patch.dict(os.environ, {"ANDROID_TARGET_SDK": "", "ANDROID_COMPILE_SDK": ""}):
            self.assertEqual(build._android_sdk_levels()[1:], (36, 36))
            for _ in range(2):
                build._configure_android_gradle_properties(self.root)
        self.assertEqual(props.read_text(), "android.useAndroidX=true\nandroid.suppressUnsupportedCompileSdk=36\n")
        with patch.dict(os.environ, {"ANDROID_TARGET_SDK": "35", "ANDROID_COMPILE_SDK": "35"}):
            build._configure_android_gradle_properties(self.root)
        self.assertEqual(props.read_text(), "android.useAndroidX=true\n")
        # The pinned NDK (ANDROID_NDK_HOME) wins over a runner image's preset ANDROID_NDK_ROOT.
        pinned, preset = self.root / "ndk/28.2.13676358", self.root / "ndk/27.3.13750724"
        pinned.mkdir(parents=True)
        preset.mkdir(parents=True)
        with patch.dict(os.environ, {"ANDROID_NDK_ROOT": str(preset), "ANDROID_NDK_HOME": str(pinned)}):
            self.assertEqual(build._detect_android_ndk_root(None), pinned)
        with patch.dict(os.environ, {"ANDROID_NDK_ROOT": str(preset), "ANDROID_NDK_HOME": "", "NDK_HOME": ""}):
            self.assertEqual(build._detect_android_ndk_root(None), preset)


if __name__ == "__main__":
    unittest.main()
