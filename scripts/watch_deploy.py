"""Device discovery, selection and command lines for the watch deploy actions
(watchos_deploy, watchos_sim_deploy, wearos_deploy, wearos_sim_deploy).

Pure helpers: nothing here runs a process. build.py runs the commands and
feeds the tool output back in, so the parsing and the error messages are
testable on any host."""
from dataclasses import dataclass
import json
from pathlib import Path
import plistlib
import re

WATCH_BUNDLE_SUFFIX = ".watchkitapp"
WEAR_ACTIVITY = "com.ubseds.gs26.wear.MainActivity"


class WatchDeployError(RuntimeError):
    """A deploy precondition failed; the message says what to do."""


# ---------------------------------------------------------------------------
# Apple Watch (physical): xcrun devicectl
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class AppleDevice:
    identifier: str          # CoreDevice identifier, what --device takes
    name: str
    kind: str                # "watch", "phone", "pad" or "other"
    udid: str = ""           # hardware UDID (what provisioning profiles list)
    model: str = ""
    os_version: str = ""
    developer_mode: str = ""  # "enabled", "disabled" or "" (unknown)
    tunnel_state: str = ""    # "connected", "disconnected", "unavailable", ...
    pairing_state: str = ""

    def label(self):
        extra = ", ".join(x for x in (self.model, self.os_version and f"OS {self.os_version}") if x)
        ids = self.identifier + (f"; UDID {self.udid}" if self.udid and self.udid != self.identifier else "")
        return f"{self.name} ({extra}; {ids})" if extra else f"{self.name} ({ids})"


def devicectl_list_command(json_path):
    return ["xcrun", "devicectl", "list", "devices", "--json-output", str(json_path)]


def _apple_kind(*values):
    text = " ".join(str(v) for v in values if v).lower()
    if "watch" in text: return "watch"
    if "ipad" in text: return "pad"
    if "iphone" in text or re.search(r"\bios\b", text): return "phone"
    return "other"


def parse_devicectl_json(text):
    """Devices from `xcrun devicectl list devices --json-output <file>`."""
    try:
        data = json.loads(text)
    except ValueError as error:
        raise WatchDeployError(f"Could not read the devicectl device list: {error}") from error
    devices = []
    for raw in ((data.get("result") or {}).get("devices") or []):
        hardware = raw.get("hardwareProperties") or {}
        props = raw.get("deviceProperties") or {}
        connection = raw.get("connectionProperties") or {}
        if hardware.get("reality") == "virtual":
            continue
        devices.append(AppleDevice(
            identifier=str(raw.get("identifier", "")),
            name=str(props.get("name") or hardware.get("marketingName") or "Apple device"),
            kind=_apple_kind(hardware.get("deviceType"), hardware.get("platform"), hardware.get("productType")),
            udid=str(hardware.get("udid", "")),
            model=str(hardware.get("marketingName") or hardware.get("productType") or ""),
            os_version=str(props.get("osVersionNumber", "")),
            developer_mode=str(props.get("developerModeStatus", "")),
            tunnel_state=str(connection.get("tunnelState", "")),
            pairing_state=str(connection.get("pairingState", "")),
        ))
    return devices


def parse_devicectl_table(text):
    """Fallback for the plain `xcrun devicectl list devices` table
    (Name, Hostname, Identifier, State, Model)."""
    devices = []
    uuid = re.compile(r"\s([0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12})\s+(.*)$")
    for line in text.splitlines():
        match = uuid.search(line)
        if not match or line.lstrip().startswith(("Name", "-")):
            continue
        head = line[:match.start()].rstrip()
        # Name and hostname are separated by two or more spaces.
        name = re.split(r"\s{2,}", head.strip())[0]
        rest = re.split(r"\s{2,}", match.group(2).strip())
        state = rest[0] if rest else ""
        model = rest[1] if len(rest) > 1 else ""
        devices.append(AppleDevice(identifier=match.group(1), name=name, kind=_apple_kind(model, name),
                                   model=model, tunnel_state=state.lower()))
    return devices


def _matches(device, wanted):
    wanted = wanted.strip().lower()
    return wanted in {device.identifier.lower(), device.udid.lower(), device.name.lower()}


def select_apple_watch(devices, wanted=None):
    """The Apple Watch to install on. `wanted` is a name, UDID or CoreDevice
    identifier (the watch=... option)."""
    watches = [d for d in devices if d.kind == "watch"]
    if wanted:
        chosen = [d for d in watches if _matches(d, wanted)]
        if not chosen:
            other = [d for d in devices if _matches(d, wanted)]
            if other:
                raise WatchDeployError(f"watch={wanted} is {other[0].label()}, not an Apple Watch.")
            raise WatchDeployError(f"No Apple Watch matches watch={wanted}. " + _watch_list_hint(watches))
        watch = chosen[0]
    elif not watches:
        raise WatchDeployError(
            "No paired Apple Watch is visible to Xcode (xcrun devicectl list devices).\n"
            "  - Pair the watch with the iPhone in the iPhone's Watch app.\n"
            "  - Connect the iPhone to this Mac (USB or same Wi-Fi) and trust the Mac on it.\n"
            "  - Unlock the watch, put it on its charger near the iPhone, and open Xcode >\n"
            "    Window > Devices and Simulators once so Xcode prepares the watch for development.\n"
            "  - To install through the iPhone only (Watch app > Available Apps), rerun with watch=skip.")
    elif len(watches) > 1:
        raise WatchDeployError("Several Apple Watches are visible; choose one with watch=<name|udid|identifier>.\n"
                               + _watch_list_hint(watches))
    else:
        watch = watches[0]
    if watch.developer_mode == "disabled":
        raise WatchDeployError(
            f"Developer Mode is off on {watch.name}. On the watch: Settings > Privacy & Security >\n"
            "  Developer Mode > On, restart it and confirm. (If the switch is missing, open Xcode >\n"
            "  Window > Devices and Simulators with the watch unlocked and on its charger first.)")
    return watch


def _watch_list_hint(watches):
    if not watches:
        return "devicectl lists no Apple Watch."
    return "Visible watches:\n" + "\n".join(f"  {w.label()}" for w in watches)


def devicectl_install_command(device_identifier, app):
    return ["xcrun", "devicectl", "device", "install", "app", "--device", device_identifier, str(app)]


def devicectl_launch_command(device_identifier, bundle_id):
    return ["xcrun", "devicectl", "device", "process", "launch", "--device", device_identifier, bundle_id]


def watch_unreachable_note(watch):
    # "disconnected" is normal for an idle watch: devicectl connects on demand.
    if watch.tunnel_state != "unavailable":
        return None
    return (f"{watch.name} is unavailable to Xcode; the direct install may fail. Unlock the watch,\n"
            "  keep it on its charger next to the iPhone, and keep the Mac, iPhone and watch on the same Wi-Fi.")


def watch_manual_install_instructions(phone_bundle_id):
    return (
        "Install the watch app from the iPhone:\n"
        "  1. Open the Watch app on the iPhone > My Watch.\n"
        f"  2. Scroll to Available Apps and tap Install next to GS26 ({phone_bundle_id}).\n"
        "     (With My Watch > General > Automatic App Install on, it installs by itself\n"
        "     a minute or two after the phone app.)\n"
        "  3. Keep the watch unlocked and on its charger until the install finishes.")


def check_watch_app_bundle(watch_app, phone_bundle_id):
    """The embedded watch app must be <phone id>.watchkitapp and name the
    phone app as its companion, or iOS refuses to pair them."""
    info_path = Path(watch_app) / "Info.plist"
    if not info_path.is_file():
        raise WatchDeployError(f"Watch app has no Info.plist: {info_path}")
    with info_path.open("rb") as f:
        info = plistlib.load(f)
    expected = phone_bundle_id + WATCH_BUNDLE_SUFFIX
    actual = info.get("CFBundleIdentifier")
    if actual != expected:
        raise WatchDeployError(
            f"Watch app bundle id is {actual!r}, expected {expected!r} (the phone app's id + {WATCH_BUNDLE_SUFFIX}).\n"
            "  Fix PRODUCT_BUNDLE_IDENTIFIER in watch/apple/project.yml or bundle.identifier in Dioxus.toml.")
    companion = info.get("WKCompanionAppBundleIdentifier")
    if companion != phone_bundle_id:
        raise WatchDeployError(
            f"Watch app WKCompanionAppBundleIdentifier is {companion!r}, expected {phone_bundle_id!r}\n"
            "  (watch/apple/GS26Watch/Info.plist).")
    return expected


def development_profile_problem(profile_info, bundle_id, matches_bundle, device_udid=None):
    """Why a decoded profile cannot dev-sign `bundle_id` (None when it can).
    `matches_bundle` is build.py's _profile_matches_bundle."""
    if not matches_bundle(profile_info, bundle_id):
        app_id = (profile_info.get("Entitlements") or {}).get("application-identifier", "?")
        return f"is for {app_id}, not {bundle_id}"
    if (profile_info.get("Entitlements") or {}).get("get-task-allow") is not True:
        return "is not a development profile (get-task-allow is not true); refusing to deploy with it"
    devices = profile_info.get("ProvisionedDevices") or []
    if not devices:
        return "has no ProvisionedDevices"
    if device_udid and device_udid.lower() not in {str(d).lower() for d in devices}:
        return (f"does not list device {device_udid}; add the device in the Apple Developer portal\n"
                "  (Certificates, Identifiers & Profiles > Devices), regenerate the profile and download it again")
    return None


# ---------------------------------------------------------------------------
# Apple Watch simulator: xcrun simctl
# ---------------------------------------------------------------------------

def simulator_target_name(machine):
    """build_targets target for the host: watchos_sim on Apple silicon,
    watchos_sim_intel on Intel Macs."""
    return "watchos_sim_intel" if machine.lower() in {"x86_64", "amd64", "i386"} else "watchos_sim"


@dataclass(frozen=True)
class SimPair:
    pair_id: str
    phone_udid: str
    phone_name: str
    phone_state: str
    watch_udid: str
    watch_name: str
    watch_state: str
    state: str

    @property
    def active(self):
        return "active" in self.state and "inactive" not in self.state


def parse_simctl_pairs(text):
    """Pairs from `xcrun simctl list pairs -j`."""
    try:
        data = json.loads(text)
    except ValueError as error:
        raise WatchDeployError(f"Could not read `simctl list pairs`: {error}") from error
    pairs = []
    for pair_id, raw in (data.get("pairs") or {}).items():
        phone, watch = raw.get("phone") or {}, raw.get("watch") or {}
        if not phone.get("udid") or not watch.get("udid"):
            continue
        pairs.append(SimPair(pair_id, str(phone["udid"]), str(phone.get("name", "")), str(phone.get("state", "")),
                             str(watch["udid"]), str(watch.get("name", "")), str(watch.get("state", "")),
                             str(raw.get("state", ""))))
    return pairs


def choose_sim_pair(pairs):
    """Prefer a pair whose devices are already booted, then the active pair,
    then the first one."""
    def score(p):
        return ((p.phone_state == "Booted") + (p.watch_state == "Booted"), p.active)
    return max(pairs, key=score) if pairs else None


def _version_key(text):
    return tuple(int(x) for x in re.findall(r"\d+", str(text)))


def choose_runtime(runtimes_json, platform_name):
    """Newest available runtime for "iOS" or "watchOS" from
    `xcrun simctl list runtimes -j`."""
    data = json.loads(runtimes_json)
    candidates = []
    for runtime in data.get("runtimes") or []:
        if runtime.get("isAvailable") is False:
            continue
        platform_value = runtime.get("platform") or str(runtime.get("name", "")).split(" ")[0]
        if platform_value != platform_name:
            continue
        candidates.append(runtime)
    if not candidates:
        raise WatchDeployError(
            f"No {platform_name} simulator runtime is installed. Install it in Xcode > Settings > Platforms\n"
            f"  (or: xcodebuild -downloadPlatform {platform_name}).")
    return max(candidates, key=lambda r: _version_key(r.get("version", "")))


def choose_device_type(runtime, family):
    """Newest device type of `family` ("iPhone" or "Apple Watch") the runtime
    supports."""
    types = [t for t in runtime.get("supportedDeviceTypes") or []
             if t.get("productFamily") == family or str(t.get("name", "")).startswith(family)]
    if not types:
        raise WatchDeployError(f"Runtime {runtime.get('name')} supports no {family} simulator.")
    return types[-1]


def simctl_create_pair_commands(phone_type, phone_runtime, watch_type, watch_runtime):
    return (["xcrun", "simctl", "create", "GS26 iPhone", phone_type, phone_runtime],
            ["xcrun", "simctl", "create", "GS26 Apple Watch", watch_type, watch_runtime])


# ---------------------------------------------------------------------------
# Wear OS: adb
# ---------------------------------------------------------------------------

def parse_adb_devices(text):
    """(serial, state) for every line of `adb devices`."""
    rows = []
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith(("List of devices attached", "*")):
            continue
        parts = line.split()
        if len(parts) >= 2:
            rows.append((parts[0], parts[1]))
    return rows


def is_emulator_serial(serial):
    return serial.startswith("emulator-")


def is_watch_characteristics(value):
    return "watch" in {part.strip() for part in value.lower().split(",")}


def select_wear_serial(rows, characteristics, *, emulator, wanted=None):
    """The Wear OS device to install on.

    rows: parse_adb_devices output. characteristics: serial ->
    ro.build.characteristics for the rows in state "device". emulator: True
    for wearos_sim_deploy (emulators only), False for wearos_deploy (physical
    watches only)."""
    kind = "Wear OS emulator" if emulator else "Wear OS watch"
    if wanted:
        states = dict(rows)
        if wanted not in states:
            raise WatchDeployError(f"adb does not list serial={wanted}. Connected: "
                                   + (", ".join(f"{s} ({st})" for s, st in rows) or "none"))
        if states[wanted] != "device":
            raise WatchDeployError(_adb_state_hint(wanted, states[wanted]))
        if not is_watch_characteristics(characteristics.get(wanted, "")):
            raise WatchDeployError(f"serial={wanted} is not a Wear OS device "
                                   f"(ro.build.characteristics={characteristics.get(wanted, '')!r}).")
        return wanted
    watches = [s for s, st in rows if st == "device" and is_emulator_serial(s) == emulator
               and is_watch_characteristics(characteristics.get(s, ""))]
    if len(watches) == 1:
        return watches[0]
    if len(watches) > 1:
        raise WatchDeployError(f"Several {kind}s are connected ({', '.join(watches)}); choose one with serial=<serial>.")
    blocked = [(s, st) for s, st in rows if st != "device" and is_emulator_serial(s) == emulator]
    if blocked:
        raise WatchDeployError(_adb_state_hint(*blocked[0]))
    raise WatchDeployError(f"No {kind} found in `adb devices`.\n" + (
        "  Start a Wear OS AVD first (Android Studio > Device Manager, or `emulator -avd <name>`)."
        if emulator else adb_wifi_instructions()))


def _adb_state_hint(serial, state):
    if state == "unauthorized":
        return f"{serial} is unauthorized: accept the debugging prompt on the watch, then rerun."
    if state == "offline":
        return f"{serial} is offline: run `adb disconnect {serial}` and `adb connect <ip>:<port>` again."
    return f"{serial} is in adb state {state!r}, not ready for install."


def adb_wifi_instructions():
    return (
        "Connect the watch over Wi-Fi debugging:\n"
        "  1. On the watch: Settings > System > About > Versions, tap Build number 7 times.\n"
        "  2. Settings > Developer options: turn on ADB debugging and Wireless debugging\n"
        "     (watch and computer on the same Wi-Fi network).\n"
        "  3. Wireless debugging > Pair new device shows a pairing code and IP:port:\n"
        "       adb pair <ip>:<pairing-port>      # enter the pairing code\n"
        "  4. The Wireless debugging screen shows the connect IP:port:\n"
        "       adb connect <ip>:<port>\n"
        "  5. `adb devices` lists the watch; rerun this command (add serial=<ip>:<port> if several devices).")


def wear_avd_names(avd_names, avd_home):
    """The AVDs (from `emulator -list-avds`) whose config is a Wear OS image."""
    wear = []
    for name in avd_names:
        config = Path(avd_home) / f"{name}.avd" / "config.ini"
        try:
            text = config.read_text(encoding="utf-8", errors="replace").lower()
        except OSError:
            continue
        if "android-wear" in text or "wear" in re.findall(r"tag\.id\s*=\s*(\S+)", text):
            wear.append(name)
    return wear


def wear_launch_command(adb, serial, application_id):
    return [adb, "-s", serial, "shell", "am", "start", "-n", f"{application_id}/{WEAR_ACTIVITY}"]
