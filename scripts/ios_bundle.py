"""Read-only iOS Mach-O preflight, before metadata changes or signing.

Inspects metadata only; this does not establish installability or signature trust.
"""
import os
from pathlib import Path
import plistlib
import struct

TARGETS = {
    'aarch64-apple-ios': (0x0100000c, 2, 'iphoneos', 'arm64-apple-ios13.0'),
    'aarch64-apple-ios-sim': (0x0100000c, 7, 'iphonesimulator', 'arm64-apple-ios13.0-simulator'),
    'x86_64-apple-ios': (0x01000007, 7, 'iphonesimulator', 'x86_64-apple-ios13.0-simulator'),
}
THIN = {b'\xcf\xfa\xed\xfe': ('<', 32), b'\xfe\xed\xfa\xcf': ('>', 32),
        b'\xce\xfa\xed\xfe': ('<', 28), b'\xfe\xed\xfa\xce': ('>', 28)}
FAT = {b'\xca\xfe\xba\xbe': ('>', False), b'\xbe\xba\xfe\xca': ('<', False),
       b'\xca\xfe\xba\xbf': ('>', True), b'\xbf\xba\xfe\xca': ('<', True)}

def target_spec(target):
    try:
        return TARGETS[target]
    except KeyError as error:
        raise RuntimeError(f'Unsupported iOS artifact target: {target}') from error

def _read(file, offset, size, end):
    if offset < 0 or size < 0 or offset + size > end:
        raise RuntimeError('Truncated or out-of-bounds Mach-O metadata')
    file.seek(offset)
    data = file.read(size)
    if len(data) != size:
        raise RuntimeError('Truncated Mach-O metadata')
    return data

def _thin(file, offset, size):
    end = offset + size
    magic = _read(file, offset, 4, end)
    if magic not in THIN:
        raise RuntimeError('Fat Mach-O slice must contain a thin Mach-O image')
    order, header_size = THIN[magic]
    header = _read(file, offset, header_size, end)
    cpu, subtype, filetype, count, command_bytes, _flags = struct.unpack_from(order + '6I', header, 4)
    if count > 65536 or command_bytes > 4 * 1024 * 1024:
        raise RuntimeError('Unreasonable Mach-O load-command metadata')
    commands = _read(file, offset + header_size, command_bytes, end)
    cursor = 0
    platforms = []
    for _ in range(count):
        if cursor + 8 > len(commands):
            raise RuntimeError('Truncated Mach-O load command')
        command, length = struct.unpack_from(order + '2I', commands, cursor)
        if length < 8 or length % 4 or cursor + length > len(commands):
            raise RuntimeError('Invalid Mach-O load-command size')
        if command == 0x32:  # LC_BUILD_VERSION, Apple mach-o/loader.h
            if length < 24:
                raise RuntimeError('Truncated LC_BUILD_VERSION')
            platform, _minimum, _sdk, tools = struct.unpack_from(order + '4I', commands, cursor + 8)
            if tools > (length - 24) // 8:
                raise RuntimeError('Truncated LC_BUILD_VERSION tool table')
            platforms.append(platform)
        cursor += length
    if cursor != len(commands) or len(platforms) != 1:
        raise RuntimeError('Exactly one LC_BUILD_VERSION is required to distinguish device from simulator')
    return {'cpu': cpu, 'subtype': subtype, 'platform': platforms[0], 'filetype': filetype}

def inspect_macho(path):
    path = Path(path)
    with path.open('rb') as file:
        size = os.fstat(file.fileno()).st_size
        magic = _read(file, 0, 4, size)
        if magic in THIN:
            return [_thin(file, 0, size)]
        if magic not in FAT:
            raise RuntimeError('Expected a Mach-O executable or dynamic library')
        order, wide = FAT[magic]
        count = struct.unpack(order + 'I', _read(file, 4, 4, size))[0]
        if not 1 <= count <= 64:
            raise RuntimeError('Invalid fat Mach-O architecture count')
        entry_size = 32 if wide else 20
        table = _read(file, 8, count * entry_size, size)
        slices, ranges = [], []
        for index in range(count):
            entry = struct.unpack_from(order + ('2I2Q2I' if wide else '5I'), table, index * entry_size)
            cpu, _subtype, offset, length = entry[:4]
            if offset < 8 + len(table) or offset + length > size:
                raise RuntimeError('Out-of-bounds fat Mach-O slice')
            if any(offset < stop and offset + length > start for start, stop in ranges):
                raise RuntimeError('Overlapping fat Mach-O slices')
            ranges.append((offset, offset + length))
            row = _thin(file, offset, length)
            if row['cpu'] != cpu:
                raise RuntimeError('Fat Mach-O CPU descriptor disagrees with its image')
            slices.append(row)
        return slices

def _inside(path, root):
    try:
        resolved = path.resolve(strict=True)
    except OSError as error:
        raise RuntimeError(f'Missing native payload: {path.name}') from error
    if not resolved.is_relative_to(root):
        raise RuntimeError(f'Native payload escapes its app bundle: {path.name}')
    return resolved

def _bundle_executable(bundle, root):
    try:
        with (bundle / 'Info.plist').open('rb') as file:
            info = plistlib.load(file)
            if not isinstance(info, dict):
                raise RuntimeError(f'Invalid bundle metadata: {bundle.name}')
            name = info.get('CFBundleExecutable')
    except (OSError, ValueError, plistlib.InvalidFileException) as error:
        raise RuntimeError(f'Cannot read executable metadata: {bundle.name}') from error
    if not isinstance(name, str) or not name or Path(name).name != name or name in {'.', '..'}:
        raise RuntimeError(f'Invalid CFBundleExecutable: {bundle.name}')
    return _inside(bundle / name, root)

def validate_app(app, target):
    app = Path(app).resolve(strict=True)
    expected_cpu, expected_platform, _sdk, _clang = target_spec(target)
    required = {_bundle_executable(app, app)}
    for parent, directories, names in os.walk(app, followlinks=False):
        parent = Path(parent)
        for name in directories:
            if name.endswith(('.framework', '.appex')):
                bundle = parent / name
                _inside(bundle, app)
                required.add(_bundle_executable(bundle, app))
        for name in names:
            path = parent / name
            if name.endswith('.dylib'):
                required.add(_inside(path, app))
            elif not path.is_symlink():
                with path.open('rb') as file:
                    magic = file.read(4)
                if magic in THIN:
                    required.add(path.resolve())
                elif magic in FAT:
                    # CAFEBABE also identifies Java resources. Declared native
                    # paths above always validate strictly; recognize other fat
                    # images only when their complete metadata is well-formed.
                    try:
                        inspect_macho(path)
                    except RuntimeError:
                        continue
                    required.add(path.resolve())
    for path in sorted(required):
        try:
            rows = inspect_macho(path)
            for row in rows:
                if row['cpu'] != expected_cpu or row['platform'] != expected_platform:
                    raise RuntimeError(f"CPU={row['cpu']:#x}, platform={row['platform']} does not match {target}")
        except (OSError, RuntimeError) as error:
            raise RuntimeError(f'Invalid iOS payload {path.relative_to(app)}: {error}. Rebuild for {target}; do not reuse a simulator/device bundle interchangeably.') from error
    return len(required)
