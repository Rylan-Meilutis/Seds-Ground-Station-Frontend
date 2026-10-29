import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("frontend_web_build", Path(__file__).parents[1] / "build.py")
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


class WebAssetPruningTests(unittest.TestCase):
    def test_current_loader_keeps_its_older_wasm_and_transitive_imports(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            public = root / "dist/public"
            assets = public / "assets"
            assets.mkdir(parents=True)
            files = {
                "app-dxha.js": 'import "/assets/glue-dxha.js";',
                "glue-dxha.js": 'new URL("/assets/app_bg-dxha.wasm", import.meta.url);',
                "app_bg-dxha.wasm": "current-wasm",
                "app_bg-dxha.wasm.gz": "current-compressed",
                "app_bg-dxhb.wasm": "newer-unreferenced-wasm",
                "app_bg-dxhc.wasm": "newest-unreferenced-wasm",
                "app_bg-dxhd.wasm": "stale-unreferenced-wasm",
            }
            for name, value in files.items():
                (assets / name).write_text(value)
            for name in ["app_bg-dxha.wasm", "app_bg-dxha.wasm.gz"]:
                os.utime(assets / name, (1, 1))
            os.utime(assets / "app_bg-dxhd.wasm", (0, 0))
            (public / "index.html").write_text('<script src="/./assets/app-dxha.js"></script>')
            with patch.dict(os.environ, {"GS_ASSET_HASH_KEEP": "1"}):
                build._prune_stale_hashed_assets(root)
            self.assertTrue((assets / "app_bg-dxha.wasm").is_file())
            self.assertTrue((assets / "app_bg-dxha.wasm.gz").is_file())
            self.assertFalse((assets / "app_bg-dxhd.wasm").exists())
            build._validate_web_asset_references(root)

    def test_missing_wasm_is_rejected_before_publication(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            public = root / "dist/public"
            assets = public / "assets"
            assets.mkdir(parents=True)
            (public / "index.html").write_text('<script src="/assets/app-dxha.js"></script>')
            (assets / "app-dxha.js").write_text('new URL("/assets/app_bg-dxha.wasm", import.meta.url);')
            with self.assertRaisesRegex(RuntimeError, "missing assets/app_bg-dxha.wasm"):
                build._validate_web_asset_references(root)


if __name__ == "__main__":
    unittest.main()
