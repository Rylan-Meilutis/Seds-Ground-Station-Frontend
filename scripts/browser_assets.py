"""Prepare frontend-owned generated browser dependencies without compiling the UI.

Only this module downloads MapLibre: Rust build scripts and build.py delegate
here. A receipt is committed last, after both atomic files have been verified.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import errno
import hashlib
from http.client import HTTPException
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import Request, urlopen

DEFAULT_VERSION = "4.7.1"
DEFAULT_BASE_URL = "https://unpkg.com"
# Verified against the official npm4.7.1 tarball after SHA512 integrity checking.
# These pins also validate existing source/cache files, not just downloads.
PINNED_SHA256 = {
    "maplibre-gl.css": "576b085fdd9487a65a19215328c1e086c07ce5bf6da09b666b3806d3d008dae9",
    "maplibre-gl.js": "be9633c4d870e26fb37f1cfe5c5a77181667114003ea16207ac7850d8da8add1",
}
ASSETS = {"maplibre-gl.css": 1024 * 1024, "maplibre-gl.js": 8 * 1024 * 1024}
RECEIPT = ".maplibre-gl-assets.json"
STAMP = ".maplibre-gl-stamp"


def default_cache() -> Path:
    explicit = os.environ.get("GS_BROWSER_ASSETS_CACHE")
    if explicit:
        return Path(explicit).expanduser()
    if sys.platform == "darwin":
        return Path.home() / "Library/Caches/GroundStation26/browser-assets"
    if os.name == "nt":
        return Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData/Local")) / "GroundStation26/browser-assets"
    return Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "groundstation26/browser-assets"


@contextmanager
def locked(path: Path):
    """Native advisory lock: released by the OS even if a build is interrupted."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+b") as handle:
        if handle.seek(0, os.SEEK_END) == 0:
            handle.write(b"\0")
            handle.flush()
        deadline = time.monotonic() + 120
        while True:
            try:
                if os.name == "nt":
                    import msvcrt
                    handle.seek(0)
                    msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
                else:
                    import fcntl
                    fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except OSError as error:
                if error.errno not in {errno.EACCES, errno.EAGAIN, errno.EDEADLK}:
                    raise
                if time.monotonic() >= deadline:
                    raise RuntimeError("Timed out waiting for browser asset preparation") from None
                time.sleep(0.05)
        try:
            yield
        finally:
            if os.name == "nt":
                import msvcrt
                handle.seek(0)
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl
                fcntl.flock(handle, fcntl.LOCK_UN)


def atomic_write(path: Path, data: bytes) -> None:
    try:
        if path.stat().st_size == len(data):
            with path.open("rb") as handle:
                if handle.read(len(data) + 1) == data:
                    return
    except FileNotFoundError:
        pass
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=".browser-asset-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        os.chmod(temporary, 0o644)
        os.replace(temporary, path)
        if os.name != "nt":
            descriptor = os.open(path.parent, os.O_RDONLY)
            try:
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
    finally:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass


def clean_interrupted_writes(directory: Path) -> None:
    # Called only while holding this directory's preparation lock. These are
    # our private atomic-write remnants, never completed assets or user files.
    for temporary in directory.glob(".browser-asset-*"):
        temporary.unlink(missing_ok=True)


def read_receipt(directory: Path, version: str, base: str) -> dict:
    try:
        with (directory / RECEIPT).open("rb") as handle:
            raw = handle.read(4097)
        if len(raw) > 4096:
            return {}
        value = json.loads(raw)
        if isinstance(value, dict) and value.get("format") == 1 and value.get("version") == version and value.get("base_url") == base:
            assets = value.get("assets", {})
            return assets if isinstance(assets, dict) else {}
    except (OSError, ValueError, AttributeError, RecursionError):
        pass
    return {}


def validate(data: bytes, name: str, expected: str | None) -> str:
    if not 128 <= len(data) <= ASSETS[name]:
        raise ValueError(f"Invalid size for {name}")
    text = data.decode("utf-8")
    if "gs26-maplibre-placeholder" in text or text.lstrip().lower().startswith(("<!doctype html", "<html")):
        raise ValueError(f"Placeholder or HTML response for {name}")
    if "maplibre" not in text.lower():
        raise ValueError(f"Unexpected content for {name}")
    digest = hashlib.sha256(data).hexdigest()
    if expected is not None and digest != expected:
        raise ValueError(f"Checksum mismatch for {name}")
    return digest


def read_valid(directory: Path, version: str, base: str) -> dict[str, bytes] | None:
    receipt = read_receipt(directory, version, base)
    result = {}
    for name, limit in ASSETS.items():
        entry = receipt.get(name, {})
        expected = PINNED_SHA256[name] if version == DEFAULT_VERSION else entry.get("sha256") if isinstance(entry, dict) else None
        if not isinstance(expected, str) or not re.fullmatch(r"[0-9a-f]{64}", expected):
            return None
        try:
            with (directory / name).open("rb") as handle:
                data = handle.read(limit + 1)
            validate(data, name, expected)
        except (OSError, ValueError, UnicodeError):
            return None
        result[name] = data
    return result


def receipt_bytes(values: dict[str, bytes], version: str, base: str) -> bytes:
    return (json.dumps({"format": 1, "version": version, "base_url": base, "assets": {
        name: {"sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)} for name, data in values.items()
    }}, sort_keys=True, indent=2) + "\n").encode()


def publish(directory: Path, values: dict[str, bytes], version: str, base: str) -> None:
    for name, data in values.items():
        atomic_write(directory / name, data)
    atomic_write(directory / RECEIPT, receipt_bytes(values, version, base))
    atomic_write(directory / STAMP, f"version={version}\n".encode())


def download(name: str, version: str, base: str) -> bytes:
    url = f"{base}/maplibre-gl@{version}/dist/{name}"
    failure = None
    for attempt in range(3):
        try:
            request = Request(url, headers={"User-Agent": "GroundStation26-browser-assets/1", "Accept-Encoding": "identity"})
            with urlopen(request, timeout=30) as response:
                final = urlsplit(response.url)
                if final.scheme != "https" and not (final.scheme == "http" and final.hostname in {"127.0.0.1", "localhost", "::1"}):
                    raise ValueError("Browser asset redirect requires HTTPS")
                allowed = {"text/plain", "application/octet-stream"}
                allowed |= {"text/css"} if name.endswith(".css") else {"application/javascript", "text/javascript", "application/x-javascript"}
                if response.headers.get_content_type() not in allowed:
                    raise ValueError(f"Unexpected response type for {name}")
                declared = response.headers.get("Content-Length")
                declared = int(declared) if declared is not None else None
                if declared is not None and not 128 <= declared <= ASSETS[name]:
                    raise ValueError(f"Invalid declared size for {name}")
                data = response.read(ASSETS[name] + 1)
                if declared is not None and len(data) != declared:
                    raise ValueError(f"Incomplete response for {name}")
            validate(data, name, PINNED_SHA256[name] if version == DEFAULT_VERSION else None)
            return data
        except (OSError, URLError, HTTPException, ValueError, UnicodeError) as error:
            failure = str(error)
            if isinstance(error, HTTPError):
                error.close()
            if attempt < 2:
                time.sleep(0.25 * (attempt + 1))
    raise RuntimeError(f"Unable to prepare {name}: {failure}. Restore network access or supply a verified GS_BROWSER_ASSETS_CACHE.") from None


def prepare(root: Path, *, cache: Path | None = None, version: str | None = None, base: str | None = None) -> Path:
    version = version if version is not None else os.environ.get("MAPLIBRE_GL_VERSION", DEFAULT_VERSION)
    base = (base if base is not None else os.environ.get("MAPLIBRE_GL_BASE_URL", DEFAULT_BASE_URL)).rstrip("/")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]{0,63}", version) or ".." in version:
        raise ValueError("Invalid MAPLIBRE_GL_VERSION")
    address = urlsplit(base)
    if address.username or address.password or address.query or address.fragment or not address.hostname:
        raise ValueError("MAPLIBRE_GL_BASE_URL must not contain credentials, queries or fragments")
    if address.scheme != "https" and not (address.scheme == "http" and address.hostname in {"127.0.0.1", "localhost", "::1"}):
        raise ValueError("Browser assets require HTTPS (HTTP is allowed only for loopback test fixtures)")
    vendor = root.resolve() / "static/vendor/maplibre-gl"
    with locked(vendor / ".prepare.lock"):
        clean_interrupted_writes(vendor)
        values = read_valid(vendor, version, base)
        if values is None:
            cache_root = (cache if cache is not None else default_cache()).expanduser().resolve()
            cache_dir = cache_root / f"maplibre-{version}-{hashlib.sha256(base.encode()).hexdigest()[:16]}"
            with locked(cache_dir / ".prepare.lock"):
                clean_interrupted_writes(cache_dir)
                values = read_valid(cache_dir, version, base)
                if values is None:
                    values = {name: download(name, version, base) for name in ASSETS}
                    publish(cache_dir, values, version, base)
                publish(vendor, values, version, base)
        else:
            # Upgrade the old stamp-only format only after checking actual bytes.
            publish(vendor, values, version, base)
    return vendor


def main(args: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--cache", type=Path)
    options = parser.parse_args(args)
    try:
        prepare(options.root, cache=options.cache)
    except (OSError, ValueError, RuntimeError) as error:
        print(f"Browser asset preparation failed: {error}", file=sys.stderr)
        return 1
    print("Frontend browser assets verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
