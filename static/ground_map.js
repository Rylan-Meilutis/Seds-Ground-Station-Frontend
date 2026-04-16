//
// frontend/assets/ground_map.js
//
// Leaflet helpers for GroundStation26
// - ES module with named exports (required by wasm-bindgen)
// - Emoji-based markers (🚀 + 🧍) with heading indicator (▲)
// - Browser compass + geolocation support
//

let groundMap = null;
let groundTileLayer = null;
let rocketMarker = null;
let userMarker = null;
let rocketGuideLine = null;

// Remember last-known positions across tab switches
let lastRocketLatLng = null;
let lastUserLatLng = null;
let lastMapView = null;
let currentTilesUrl = null;
let currentMaxNativeZoom = null;
let currentMaxZoom = null;
let tileCacheSweepTimer = null;
let tilePrefetchTimer = null;
let currentPrefetchKey = null;
let tilePrefetchRunId = 0;
let followUserEnabled = false;
let orientationMode = "north";
let mapBearingDeg = 0;
let suppressFollowDisableUntilMs = 0;
let followEnableGuardUntilMs = 0;
let tilePrefetchState = {
    key: "",
    state: "idle",
    pending: 0,
    completed: 0,
    failed: 0,
    lastStartedAt: 0,
    lastCompletedAt: 0,
};

// you currently have tiles for z = 0..8
const MIN_ZOOM = 0;
const DEFAULT_MAX_NATIVE_ZOOM = 12;
const DEFAULT_MAX_OVERZOOM_DELTA = 1;
const HIGH_RES_PREFETCH_RADIUS_M = 1609.344;
const HIGH_RES_PREFETCH_MIN_ZOOM_DELTA = 1;
const HIGH_RES_PREFETCH_MAX_TILES = 192;
const HIGH_RES_PREFETCH_CONCURRENCY = 6;

// Must match Rust NA_BOUNDS in build.rs
const NA_BOUNDS = {
    lonMin: -170.0,
    latMin: 5.0,
    lonMax: -50.0,
    latMax: 83.0,
};

// =========================
// Marker sizing (TUNE HERE)
// =========================
const MARKER_PX = 48;                 // overall icon box
const MARKER_ANCHOR = MARKER_PX / 2;  // center anchor
const USER_FONT_PX = 25;              // 🧍 size
const ROCKET_FONT_PX = 25;            // 🚀 size
const ARROW_FONT_PX = 18;             // ▲ size
const ARROW_RADIUS = Math.round(MARKER_PX * 0.5) - 1; // ▲ distance from center

// Raw + filtered heading (0..360, 0 = North)
let userHeadingDegRaw = null;
let userHeadingDeg = null;
let nativeHeadingDeg = null;
let deviceHeadingDeg = null;
let compassInitialized = false;

// ============================================================================
// Utilities
// ============================================================================

function getLeaflet() {
    if (typeof L === "undefined") {
        throw new Error(
            "Leaflet global `L` is not defined. Load leaflet.js before wasm."
        );
    }
    return L;
}

function normalizeAngle(deg) {
    let d = deg % 360;
    if (d < 0) d += 360;
    return d;
}

function shortestAngleDiff(a, b) {
    let diff = normalizeAngle(b) - normalizeAngle(a);
    if (diff > 180) diff -= 360;
    if (diff < -180) diff += 360;
    return diff;
}

function circularMeanDeg(a, b, wa, wb) {
    const ar = normalizeAngle(a) * Math.PI / 180.0;
    const br = normalizeAngle(b) * Math.PI / 180.0;
    const x = Math.cos(ar) * wa + Math.cos(br) * wb;
    const y = Math.sin(ar) * wa + Math.sin(br) * wb;
    if (!Number.isFinite(x) || !Number.isFinite(y) || (x === 0 && y === 0)) {
        return normalizeAngle(a);
    }
    return normalizeAngle(Math.atan2(y, x) * 180.0 / Math.PI);
}

function fusedHeadingTarget() {
    const hasNative = Number.isFinite(nativeHeadingDeg);
    const hasDevice = Number.isFinite(deviceHeadingDeg);
    // Native mobile heading is already north-referenced and posture-independent.
    // Do not blend it with browser/deviceorientation data, which can vary with
    // screen posture and reintroduce orientation-dependent drift.
    if (hasNative) return normalizeAngle(nativeHeadingDeg);
    if (hasDevice) return normalizeAngle(deviceHeadingDeg);
    return null;
}

function applyFusedHeading() {
    const target = fusedHeadingTarget();
    if (!Number.isFinite(target)) return;

    userHeadingDegRaw = target;

    if (!Number.isFinite(userHeadingDeg)) {
        userHeadingDeg = target;
    } else {
        const diff = shortestAngleDiff(userHeadingDeg, target);
        const gain = Number.isFinite(nativeHeadingDeg)
            ? Math.min(0.92, Math.max(0.72, Math.abs(diff) / 45.0))
            : Math.min(0.55, Math.max(0.16, Math.abs(diff) / 90.0));
        userHeadingDeg = normalizeAngle(userHeadingDeg + diff * gain);
    }

    updateUserMarkerRotation();
    if (followUserEnabled && orientationMode === "user") {
        mapBearingDeg = normalizeAngle(userHeadingDeg);
        persistMapState();
        applyMapOrientation();
    }
}

function markerCounterRotationDeg() {
    return normalizeAngle(mapBearingDeg);
}

function updateMarkerCounterRotation(marker) {
    if (!marker) return;
    const el = marker.getElement();
    if (!el) return;
    const wrapper = el.querySelector(".user-marker-wrapper");
    if (!wrapper) return;
    wrapper.style.transform = `rotate(${markerCounterRotationDeg()}deg)`;
}

function updateAllMarkerCounterRotation() {
    updateMarkerCounterRotation(rocketMarker);
    updateMarkerCounterRotation(userMarker);
    updateUserMarkerRotation();
}

function applyMapOrientation() {
    if (!groundMap) return;
    const pane = groundMap.getPane && groundMap.getPane("mapPane");
    if (!pane) return;

    const rotateDeg = normalizeAngle(mapBearingDeg);

    pane.style.transformOrigin = "50% 50%";
    pane.style.rotate = rotateDeg ? `${-rotateDeg}deg` : "";
    updateAllMarkerCounterRotation();
}

function markerVisualCenter(marker) {
    if (!marker) return null;
    const el = marker.getElement();
    if (!el) return null;
    const rect = el.getBoundingClientRect();
    if (!Number.isFinite(rect.left) || !Number.isFinite(rect.top)) return null;
    return {
        x: rect.left + rect.width / 2.0,
        y: rect.top + rect.height / 2.0,
    };
}

function mapVisualCenter() {
    if (!groundMap) return null;
    const container = groundMap.getContainer();
    if (!container) return null;
    const rect = container.getBoundingClientRect();
    if (!Number.isFinite(rect.left) || !Number.isFinite(rect.top)) return null;
    return {
        x: rect.left + rect.width / 2.0,
        y: rect.top + rect.height / 2.0,
    };
}

function correctVisualCenterOnMarker(marker, attemptsLeft = 4) {
    if (!groundMap || !marker || attemptsLeft <= 0) return;
    requestAnimationFrame(() => {
        try {
            const markerCenter = markerVisualCenter(marker);
            const center = mapVisualCenter();
            if (!markerCenter || !center) return;
            const dx = markerCenter.x - center.x;
            const dy = markerCenter.y - center.y;
            if (Math.abs(dx) > 1.0 || Math.abs(dy) > 1.0) {
                suppressFollowDisableUntilMs = Date.now() + 750;
                groundMap.panBy([dx, dy], {animate: false});
                correctVisualCenterOnMarker(marker, attemptsLeft - 1);
            }
        } catch (e) {
        }
    });
}

function setGroundMapOrientationMode(mode) {
    orientationMode = mode === "user" ? "user" : (mode === "manual" ? "manual" : "north");
    if (orientationMode === "north") {
        mapBearingDeg = 0;
    } else if (orientationMode === "user" && followUserEnabled && Number.isFinite(userHeadingDeg)) {
        mapBearingDeg = normalizeAngle(userHeadingDeg);
    }
    persistMapState();
    applyMapOrientation();
}

function adjustGroundMapBearing(deltaDeg) {
    const delta = Number(deltaDeg);
    if (!Number.isFinite(delta)) return;
    orientationMode = "manual";
    mapBearingDeg = normalizeAngle(mapBearingDeg + delta);
    persistMapState();
    applyMapOrientation();
}

function setGroundMapBearing(deg) {
    if (!Number.isFinite(deg)) return;
    orientationMode = "manual";
    mapBearingDeg = normalizeAngle(deg);
    persistMapState();
    applyMapOrientation();
}

function setGroundMapFollowUser(enabled) {
    followUserEnabled = enabled === true;
    followEnableGuardUntilMs = followUserEnabled ? Date.now() + 1500 : 0;
    try {
        window.__gs26_follow_user_enabled = followUserEnabled ? "true" : "false";
        window.__gs26_follow_user_enable_guard_until = followEnableGuardUntilMs;
        window.dispatchEvent(new CustomEvent("gs26-follow-user-changed", {
            detail: {enabled: followUserEnabled}
        }));
    } catch (e) {
    }
    if (followUserEnabled && groundMap && lastUserLatLng) {
        suppressFollowDisableUntilMs = Date.now() + 750;
        groundMap.setView(lastUserLatLng, groundMap.getZoom(), {animate: false});
        correctVisualCenterOnMarker(userMarker);
        scheduleHighResTilePrefetch();
    }
    if (followUserEnabled && orientationMode === "user" && Number.isFinite(userHeadingDeg)) {
        mapBearingDeg = normalizeAngle(userHeadingDeg);
    }
    persistMapState();
    applyMapOrientation();
}

function disableFollowUserFromMapInteraction() {
    if (Date.now() < suppressFollowDisableUntilMs) return;
    if (!followUserEnabled) return;
    followUserEnabled = false;
    followEnableGuardUntilMs = 0;
    if (orientationMode === "user") {
        orientationMode = "manual";
    }
    persistMapState();
    try {
        window.__gs26_follow_user_enabled = "false";
        window.__gs26_follow_user_enable_guard_until = 0;
        window.__gs26_map_orientation_mode = orientationMode;
        window.dispatchEvent(new CustomEvent("gs26-follow-user-changed", {
            detail: {enabled: false}
        }));
    } catch (e) {
    }
    applyMapOrientation();
}

// ============================================================================
// CSS injection (runs once)
// ============================================================================

function ensureMarkerStylesOnce() {
    if (document.getElementById("gs26-marker-styles")) return;

    const style = document.createElement("style");
    style.id = "gs26-marker-styles";
    style.textContent = `
    .user-marker-wrapper {
      position: relative;
      width: ${MARKER_PX}px;
      height: ${MARKER_PX}px;
      pointer-events: none;
      transform-origin: 50% 50%;
    }

    .emoji-marker {
      position: absolute;
      left: 50%;
      top: 50%;
      transform: translate(-50%, -50%);
      line-height: 1;
      user-select: none;
      filter: drop-shadow(0 2px 2px rgba(0,0,0,0.55));
    }

    .user-base {
      font-size: ${USER_FONT_PX}px;
    }

    .rocket-marker {
      font-size: ${ROCKET_FONT_PX}px;
    }

    .user-heading-indicator {
      font-size: ${ARROW_FONT_PX}px;
      transform: translate(-50%, -50%) translateY(-${ARROW_RADIUS}px);
    }
  `;
    document.head.appendChild(style);
}

// ============================================================================
// Tile helpers
// ============================================================================

function clampMaxNativeZoom(value) {
    if (!Number.isFinite(value)) return DEFAULT_MAX_NATIVE_ZOOM;
    const z = Math.floor(value);
    return Math.max(MIN_ZOOM, z);
}

function tileCacheSupported() {
    return typeof window !== "undefined"
        && typeof window.caches !== "undefined"
        && typeof window.fetch === "function";
}

function requestPersistentTileStorage() {
    try {
        if (!navigator.storage || typeof navigator.storage.persist !== "function") return;
        if (window.__gs26_tile_storage_persist_requested) return;
        window.__gs26_tile_storage_persist_requested = true;
        navigator.storage.persist().catch(() => {});
    } catch (e) {
    }
}

function tileCacheName(tilesUrl) {
    return `gs26-tiles-v1:${String(tilesUrl || "")
        .replace(/[^a-z0-9]+/gi, "_")
        .replace(/^_+|_+$/g, "")
        .toLowerCase() || "default"}`;
}

function objectUrlFromBlob(blob) {
    try {
        return URL.createObjectURL(blob);
    } catch (e) {
        console.warn("[GS26 map] failed to create tile object URL", e);
        return "";
    }
}

async function readCachedTileBlob(cacheName, url) {
    if (!tileCacheSupported() || !url) return null;
    try {
        const cache = await caches.open(cacheName);
        const cached = await cache.match(url, {ignoreVary: true});
        if (!cached || !cached.ok) return null;
        return await cached.blob();
    } catch (e) {
        console.warn("[GS26 map] cache read failed", url, e);
        return null;
    }
}

async function fetchAndCacheTileBlob(cacheName, url) {
    if (!url) return null;

    if (!tileCacheSupported()) {
        const response = await fetch(url);
        if (!response.ok) throw new Error(`tile fetch failed: ${response.status}`);
        return await response.blob();
    }

    const cache = await caches.open(cacheName);
    const cached = await cache.match(url, {ignoreVary: true});
    if (cached && cached.ok) {
        return await cached.blob();
    }

    const response = await fetch(url);
    if (!response.ok) throw new Error(`tile fetch failed: ${response.status}`);
    try {
        await cache.put(url, response.clone());
    } catch (e) {
        console.warn("[GS26 map] cache put failed", url, e);
    }
    return await response.blob();
}

function metersPerDegreeLat() {
    return 111_320.0;
}

function metersPerDegreeLon(lat) {
    const cosLat = Math.cos((lat * Math.PI) / 180.0);
    return Math.max(1.0, 111_320.0 * Math.max(0.01, Math.abs(cosLat)));
}

function clampLat(lat) {
    return Math.max(-85.05112878, Math.min(85.05112878, lat));
}

function clampLon(lon) {
    let value = lon;
    while (value < -180.0) value += 360.0;
    while (value > 180.0) value -= 360.0;
    return value;
}

function latLonToTileXY(lat, lon, zoom) {
    const scale = Math.pow(2, zoom);
    const clampedLat = clampLat(lat);
    const clampedLon = clampLon(lon);
    const latRad = clampedLat * Math.PI / 180.0;
    const x = Math.floor(((clampedLon + 180.0) / 360.0) * scale);
    const y = Math.floor(
        ((1.0 - Math.log(Math.tan(latRad) + 1.0 / Math.cos(latRad)) / Math.PI) / 2.0) * scale
    );
    return {
        x: Math.max(0, Math.min(scale - 1, x)),
        y: Math.max(0, Math.min(scale - 1, y)),
    };
}

function tileCoordsAround(lat, lon, zoom, radiusM) {
    const dLat = radiusM / metersPerDegreeLat();
    const dLon = radiusM / metersPerDegreeLon(lat);
    const north = lat + dLat;
    const south = lat - dLat;
    const east = lon + dLon;
    const west = lon - dLon;
    const nw = latLonToTileXY(north, west, zoom);
    const se = latLonToTileXY(south, east, zoom);
    const coords = [];
    for (let x = nw.x; x <= se.x; x++) {
        for (let y = nw.y; y <= se.y; y++) {
            coords.push({x, y, z: zoom});
        }
    }
    return coords;
}

function prefetchZoomLevels(maxNativeZoom) {
    const top = clampMaxNativeZoom(maxNativeZoom);
    const min = Math.max(MIN_ZOOM, top - HIGH_RES_PREFETCH_MIN_ZOOM_DELTA);
    const zooms = [];
    for (let z = top; z >= min; z--) {
        zooms.push(z);
    }
    return zooms;
}

function setTilePrefetchState(next) {
    tilePrefetchState = {
        ...tilePrefetchState,
        ...next,
    };
    try {
        window.__gs26_ground_map_cache_state = {...tilePrefetchState};
        window.__gs26_ground_map_cache_ready = tilePrefetchState.state === "ready";
    } catch (e) {
    }
}

const MAP_STATE_STORAGE_KEY = "gs26_ground_map_state_v2";

function loadPersistedMapState() {
    try {
        if (!window.localStorage) return;
        const raw = window.localStorage.getItem(MAP_STATE_STORAGE_KEY);
        if (!raw) return;
        const parsed = JSON.parse(raw);
        if (Number.isFinite(parsed.lat) && Number.isFinite(parsed.lon) && Number.isFinite(parsed.zoom)) {
            lastMapView = {lat: parsed.lat, lon: parsed.lon, zoom: parsed.zoom};
        }
        if (parsed.orientationMode === "manual" || parsed.orientationMode === "user" || parsed.orientationMode === "north") {
            orientationMode = parsed.orientationMode;
        }
        if (Number.isFinite(parsed.bearingDeg)) {
            mapBearingDeg = normalizeAngle(parsed.bearingDeg);
        }
    } catch (e) {
    }
}

function persistMapState() {
    try {
        if (!window.localStorage) return;
        const payload = {
            lat: lastMapView && Number.isFinite(lastMapView.lat) ? lastMapView.lat : null,
            lon: lastMapView && Number.isFinite(lastMapView.lon) ? lastMapView.lon : null,
            zoom: lastMapView && Number.isFinite(lastMapView.zoom) ? lastMapView.zoom : null,
            orientationMode,
            bearingDeg: mapBearingDeg,
        };
        window.localStorage.setItem(MAP_STATE_STORAGE_KEY, JSON.stringify(payload));
    } catch (e) {
    }
}

loadPersistedMapState();

function expandBoundsByRadius(bounds, radiusM) {
    if (!bounds) return null;
    const north = bounds.getNorth();
    const south = bounds.getSouth();
    const east = bounds.getEast();
    const west = bounds.getWest();
    const midLat = (north + south) / 2.0;
    const dLat = radiusM / metersPerDegreeLat();
    const dLon = radiusM / metersPerDegreeLon(midLat);
    return {
        north: clampLat(north + dLat),
        south: clampLat(south - dLat),
        east: clampLon(east + dLon),
        west: clampLon(west - dLon),
    };
}

function tileCoordsForBounds(bounds, zoom) {
    if (!bounds) return [];
    const nw = latLonToTileXY(bounds.north, bounds.west, zoom);
    const se = latLonToTileXY(bounds.south, bounds.east, zoom);
    const coords = [];
    for (let x = nw.x; x <= se.x; x++) {
        for (let y = nw.y; y <= se.y; y++) {
            coords.push({x, y, z: zoom});
        }
    }
    return coords;
}

function appendUniqueCoords(target, seen, coords, maxTiles) {
    for (const coord of coords) {
        const id = `${coord.z}/${coord.x}/${coord.y}`;
        if (seen.has(id)) continue;
        seen.add(id);
        target.push(coord);
        if (target.length >= maxTiles) break;
    }
}

function buildHighResPrefetchPlan() {
    if (!groundTileLayer || !currentTilesUrl) {
        return {key: "", coords: []};
    }

    const zooms = prefetchZoomLevels(currentMaxNativeZoom);
    const coords = [];
    const seen = new Set();
    const userLat = Array.isArray(lastUserLatLng) ? lastUserLatLng[0] : NaN;
    const userLon = Array.isArray(lastUserLatLng) ? lastUserLatLng[1] : NaN;
    const rocketLat = Array.isArray(lastRocketLatLng) ? lastRocketLatLng[0] : NaN;
    const rocketLon = Array.isArray(lastRocketLatLng) ? lastRocketLatLng[1] : NaN;
    const viewport = groundMap
        ? expandBoundsByRadius(groundMap.getBounds(), HIGH_RES_PREFETCH_RADIUS_M)
        : null;
    const viewportKey = viewport
        ? [
            viewport.north.toFixed(4),
            viewport.south.toFixed(4),
            viewport.east.toFixed(4),
            viewport.west.toFixed(4),
        ].join(",")
        : "";
    const key = [
        currentTilesUrl || "",
        String(currentMaxNativeZoom || ""),
        Number.isFinite(userLat) ? userLat.toFixed(4) : "",
        Number.isFinite(userLon) ? userLon.toFixed(4) : "",
        Number.isFinite(rocketLat) ? rocketLat.toFixed(4) : "",
        Number.isFinite(rocketLon) ? rocketLon.toFixed(4) : "",
        viewportKey,
    ].join("|");

    for (const zoom of zooms) {
        if (Number.isFinite(userLat) && Number.isFinite(userLon) && coords.length < HIGH_RES_PREFETCH_MAX_TILES) {
            appendUniqueCoords(
                coords,
                seen,
                tileCoordsAround(userLat, userLon, zoom, HIGH_RES_PREFETCH_RADIUS_M),
                HIGH_RES_PREFETCH_MAX_TILES
            );
        }

        if (viewport && coords.length < HIGH_RES_PREFETCH_MAX_TILES) {
            appendUniqueCoords(
                coords,
                seen,
                tileCoordsForBounds(viewport, zoom),
                HIGH_RES_PREFETCH_MAX_TILES
            );
        }

        if (Number.isFinite(rocketLat) && Number.isFinite(rocketLon) && coords.length < HIGH_RES_PREFETCH_MAX_TILES) {
            appendUniqueCoords(
                coords,
                seen,
                tileCoordsAround(rocketLat, rocketLon, zoom, HIGH_RES_PREFETCH_RADIUS_M),
                HIGH_RES_PREFETCH_MAX_TILES
            );
        }
    }

    return {key, coords};
}

async function runHighResTilePrefetch(runId, key) {
    const layer = groundTileLayer;
    const tilesUrl = currentTilesUrl;
    if (!layer || !tilesUrl) return;

    const plan = buildHighResPrefetchPlan();
    if (!plan.coords.length) {
        setTilePrefetchState({
            key,
            state: "ready",
            pending: 0,
            completed: 0,
            failed: 0,
            lastCompletedAt: Date.now(),
        });
        return;
    }

    const cacheName = tileCacheName(tilesUrl);
    let nextIndex = 0;
    let completed = 0;
    let failed = 0;
    const total = plan.coords.length;

    setTilePrefetchState({
        key,
        state: "warming",
        pending: total,
        completed: 0,
        failed: 0,
        lastStartedAt: Date.now(),
    });

    const worker = async () => {
        while (true) {
            if (runId !== tilePrefetchRunId || groundTileLayer !== layer || currentTilesUrl !== tilesUrl) {
                return;
            }
            const index = nextIndex++;
            if (index >= total) return;
            const coord = plan.coords[index];
            try {
                const url = layer.getTileUrl(coord);
                if (url) {
                    await fetchAndCacheTileBlob(cacheName, url);
                }
            } catch (e) {
                failed += 1;
            } finally {
                completed += 1;
                if (runId === tilePrefetchRunId) {
                    setTilePrefetchState({
                        key,
                        state: completed >= total ? "ready" : "warming",
                        pending: Math.max(0, total - completed),
                        completed,
                        failed,
                        lastCompletedAt: completed >= total ? Date.now() : tilePrefetchState.lastCompletedAt,
                    });
                }
            }
        }
    };

    const concurrency = Math.max(1, Math.min(HIGH_RES_PREFETCH_CONCURRENCY, total));
    await Promise.allSettled(Array.from({length: concurrency}, () => worker()));

    if (runId === tilePrefetchRunId) {
        setTilePrefetchState({
            key,
            state: "ready",
            pending: 0,
            completed,
            failed,
            lastCompletedAt: Date.now(),
        });
    }
}

function scheduleTileCacheSweep(tilesUrl) {
    if (!tileCacheSupported()) return;
    if (tileCacheSweepTimer) clearTimeout(tileCacheSweepTimer);
    tileCacheSweepTimer = setTimeout(async () => {
        try {
            const active = tileCacheName(tilesUrl);
            const keys = await caches.keys();
            await Promise.all(
                keys
                    .filter((key) => key.startsWith("gs26-tiles-v1:") && key !== active)
                    .map((key) => caches.delete(key))
            );
        } catch (e) {
            console.warn("[GS26 map] cache sweep failed", e);
        }
    }, 1000);
}

function scheduleHighResTilePrefetch() {
    if (!groundTileLayer || !currentTilesUrl) return;
    if (!tileCacheSupported()) return;

    const plan = buildHighResPrefetchPlan();
    const key = plan.key;
    if (!key) return;
    if (currentPrefetchKey === key) return;
    currentPrefetchKey = key;

    if (tilePrefetchTimer) clearTimeout(tilePrefetchTimer);
    const runId = ++tilePrefetchRunId;
    setTilePrefetchState({
        key,
        state: "queued",
        pending: plan.coords.length,
        completed: 0,
        failed: 0,
    });
    tilePrefetchTimer = setTimeout(async () => {
        await runHighResTilePrefetch(runId, key);
    }, 350);
}

function wireTileElement(tile, cacheName, url, done) {
    let objectUrl = null;
    const cleanup = () => {
        if (!objectUrl) return;
        try {
            URL.revokeObjectURL(objectUrl);
        } catch (e) {
        }
        objectUrl = null;
    };

    tile.onload = () => {
        cleanup();
        done(null, tile);
    };
    tile.onerror = (err) => {
        cleanup();
        done(err || new Error("tile load failed"), tile);
    };

    fetchAndCacheTileBlob(cacheName, url)
        .then((blob) => {
            if (!blob) throw new Error("tile blob missing");
            objectUrl = objectUrlFromBlob(blob);
            tile.src = objectUrl || url;
        })
        .catch(async () => {
            const cachedBlob = await readCachedTileBlob(cacheName, url);
            objectUrl = cachedBlob ? objectUrlFromBlob(cachedBlob) : "";
            tile.src = objectUrl || url;
        });
}

function createNaTileLayer(tilesUrl, maxNativeZoom, maxZoom) {
    const L = getLeaflet();

    const naBoundsLatLng = L.latLngBounds(
        [NA_BOUNDS.latMin, NA_BOUNDS.lonMin],
        [NA_BOUNDS.latMax, NA_BOUNDS.lonMax]
    );

    const layer = L.tileLayer(tilesUrl, {
        bounds: naBoundsLatLng,
        minZoom: MIN_ZOOM,
        maxZoom: maxZoom,
        maxNativeZoom: maxNativeZoom,
        noWrap: true,
        attribution: "Local tiles",
    });
    const cacheName = tileCacheName(tilesUrl);
    const originalGetTileUrl = layer.getTileUrl.bind(layer);

    layer.createTile = function (coords, done) {
        const tile = document.createElement("img");
        tile.alt = "";
        tile.setAttribute("role", "presentation");
        tile.decoding = "async";
        tile.referrerPolicy = "no-referrer";

        const url = originalGetTileUrl(coords);
        if (!tileCacheSupported()) {
            tile.onload = () => done(null, tile);
            tile.onerror = (err) => done(err || new Error("tile load failed"), tile);
            tile.src = url;
            return tile;
        }

        wireTileElement(tile, cacheName, url, done);
        return tile;
    };

    try {
        console.log("[GS26 map] tile layer created", {
            tilesUrl,
            maxNativeZoom,
            maxZoom,
        });
        layer.on("loading", () => console.log("[GS26 map] tiles loading"));
        layer.on("tileloadstart", (e) => console.log("[GS26 map] tileloadstart", e?.url || ""));
        layer.on("tileload", (e) => console.log("[GS26 map] tileload", e?.tile?.src || e?.url || ""));
        layer.on("tileerror", (e) => console.warn("[GS26 map] tileerror", e?.tile?.src || e?.url || "", e));
    } catch (e) {
        console.warn("[GS26 map] failed to install tile logging", e);
    }

    scheduleTileCacheSweep(tilesUrl);

    return layer;
}

function rememberMapView() {
    if (!groundMap) return;
    const c = groundMap.getCenter();
    lastMapView = {lat: c.lat, lon: c.lng, zoom: groundMap.getZoom()};
    persistMapState();
}

// ============================================================================
// Marker creation
// ============================================================================

function makeEmojiIcon(char, extraClass) {
    const L = getLeaflet();
    ensureMarkerStylesOnce();

    return L.divIcon({
        html: `
      <div class="user-marker-wrapper">
        <span class="emoji-marker ${extraClass || ""}">${char}</span>
      </div>
    `,
        className: "",
        iconSize: [MARKER_PX, MARKER_PX],
        iconAnchor: [MARKER_ANCHOR, MARKER_ANCHOR],
    });
}

function makeUserIcon() {
    const L = getLeaflet();
    ensureMarkerStylesOnce();

    return L.divIcon({
        html: `
      <div class="user-marker-wrapper">
        <span class="emoji-marker user-base">🧍</span>
        <span class="emoji-marker user-heading-indicator">▲</span>
      </div>
    `,
        className: "",
        iconSize: [MARKER_PX, MARKER_PX],
        iconAnchor: [MARKER_ANCHOR, MARKER_ANCHOR],
    });
}

function updateUserMarkerRotation() {
    if (!userMarker || userHeadingDeg == null) return;

    const el = userMarker.getElement();
    if (!el) return;

    const arrow = el.querySelector(".user-heading-indicator");
    if (!arrow) return;

    const relativeHeading = normalizeAngle(userHeadingDeg - mapBearingDeg);
    arrow.style.transform =
        `translate(-50%, -50%) rotate(${relativeHeading}deg) translateY(-${ARROW_RADIUS}px)`;
}

function setGroundMapUserHeading(deg) {
    if (!Number.isFinite(deg)) return;
    nativeHeadingDeg = normalizeAngle(deg);
    applyFusedHeading();
}

function syncRocketGuideLine(rocketLatLng, userLatLng) {
    if (!groundMap) return;
    const L = getLeaflet();

    if (!rocketLatLng || !userLatLng) {
        if (rocketGuideLine) {
            try {
                groundMap.removeLayer(rocketGuideLine);
            } catch (e) {
            }
            rocketGuideLine = null;
        }
        return;
    }

    const points = [userLatLng, rocketLatLng];
    if (!rocketGuideLine) {
        rocketGuideLine = L.polyline(points, {
            color: "#ef4444",
            weight: 3,
            opacity: 0.95,
        }).addTo(groundMap);
        return;
    }

    rocketGuideLine.setLatLngs(points);
}

// ============================================================================
// Compass handling
// ============================================================================

function handleOrientation(event) {
    let heading = null;

    if (typeof event.webkitCompassHeading === "number") {
        heading = normalizeAngle(event.webkitCompassHeading);
    } else if (event.absolute === true && typeof event.alpha === "number") {
        heading = normalizeAngle(event.alpha);
    } else if (typeof event.alpha === "number") {
        heading = normalizeAngle(360 - event.alpha);
    }

    if (heading == null) return;
    deviceHeadingDeg = heading;
    applyFusedHeading();
}

function initCompassOnce() {
    if (compassInitialized) return;
    compassInitialized = true;
    if (window.__gs26_disable_compass === true) return;

    if (!window.DeviceOrientationEvent) return;

    const Dev = DeviceOrientationEvent;
    if (typeof Dev.requestPermission === "function") {
        const KEY = "gs26_compass_permission_v1";

        let saved;
        try {
            saved = window.localStorage ? (window.localStorage.getItem(KEY) || "") : "";
        } catch (e) {
            saved = "";
        }

        if (saved === "granted") {
            window.addEventListener("deviceorientation", handleOrientation);
            return;
        }
        if (saved === "denied") {
            return;
        }

        Dev.requestPermission()
            .then((s) => {
                try {
                    if (window.localStorage) window.localStorage.setItem(KEY, s || "denied");
                } catch (e) {
                }
                if (s === "granted") {
                    window.addEventListener("deviceorientation", handleOrientation);
                }
            })
            .catch(() => {
                try {
                    if (window.localStorage) window.localStorage.setItem(KEY, "denied");
                } catch (e) {
                }
            });
    } else {
        window.addEventListener("deviceorientation", handleOrientation);
    }
}

// ============================================================================
// wasm-bindgen exports
// ============================================================================

function centerGroundMapOn(lat, lon) {
    if (!groundMap) return;
    suppressFollowDisableUntilMs = Date.now() + 750;
    groundMap.setView([lat, lon], groundMap.getZoom(), {animate: false});
    applyMapOrientation();
    correctVisualCenterOnMarker(userMarker);
    scheduleHighResTilePrefetch();
}

function getLastUserLatLng() {
    if (!lastUserLatLng) return null;
    return {lat: lastUserLatLng[0], lon: lastUserLatLng[1]};
}

function trackedAssetTitle() {
    return window.__gs26_tracked_asset_title || "Tracked Asset";
}

function initGroundMap(tilesUrl, centerLat, centerLon, zoom, maxNativeZoom, assetTitle) {
    const L = getLeaflet();
    ensureMarkerStylesOnce();
    requestPersistentTileStorage();
    initCompassOnce();
    window.__gs26_tracked_asset_title = assetTitle || trackedAssetTitle();
    const effectiveMaxNativeZoom = clampMaxNativeZoom(maxNativeZoom);
    const effectiveMaxZoom = effectiveMaxNativeZoom + DEFAULT_MAX_OVERZOOM_DELTA;
    const desiredZoom = lastMapView ? lastMapView.zoom : zoom;
    const clampedZoom = Math.min(effectiveMaxZoom, Math.max(MIN_ZOOM, desiredZoom));

    const el = document.getElementById("ground-map");
    if (!el) return;

    try {
        console.log("[GS26 map] initGroundMap", {
            tilesUrl,
            centerLat,
            centerLon,
            zoom,
            maxNativeZoom,
        });
    } catch (e) {
    }

    if (groundMap && groundMap.getContainer() === el) {
        const configChanged =
            currentTilesUrl !== tilesUrl ||
            currentMaxNativeZoom !== effectiveMaxNativeZoom ||
            currentMaxZoom !== effectiveMaxZoom;

        if (configChanged) {
            if (groundTileLayer) {
                try {
                    groundMap.removeLayer(groundTileLayer);
                } catch (e) {
                }
            }

            groundMap.setMinZoom(MIN_ZOOM);
            groundMap.setMaxZoom(effectiveMaxZoom);
            groundTileLayer = createNaTileLayer(
                tilesUrl,
                effectiveMaxNativeZoom,
                effectiveMaxZoom
            );
            groundTileLayer.addTo(groundMap);
            currentTilesUrl = tilesUrl;
            currentMaxNativeZoom = effectiveMaxNativeZoom;
            currentMaxZoom = effectiveMaxZoom;
            scheduleHighResTilePrefetch();

            const nextZoom = Math.min(
                effectiveMaxZoom,
                Math.max(MIN_ZOOM, groundMap.getZoom())
            );
            if (nextZoom !== groundMap.getZoom()) {
                groundMap.setZoom(nextZoom);
            }
        }

        try {
            groundMap.invalidateSize();
        } catch (e) {
        }
        return;
    }
    if (groundMap) {
        groundMap.remove();
        window.__gs26_ground_map = null;
        groundTileLayer = null;
        rocketGuideLine = null;
    }

    groundMap = L.map(el, {
        center: lastMapView ? [lastMapView.lat, lastMapView.lon] : [centerLat, centerLon],
        zoom: clampedZoom,
        minZoom: MIN_ZOOM,
        maxZoom: effectiveMaxZoom,
    });

    groundTileLayer = createNaTileLayer(tilesUrl, effectiveMaxNativeZoom, effectiveMaxZoom);
    groundTileLayer.addTo(groundMap);
    currentTilesUrl = tilesUrl;
    currentMaxNativeZoom = effectiveMaxNativeZoom;
    currentMaxZoom = effectiveMaxZoom;
    groundMap.on("moveend zoomend", () => {
        rememberMapView();
        scheduleHighResTilePrefetch();
        applyMapOrientation();
    });
    groundMap.on("dragstart zoomstart", disableFollowUserFromMapInteraction);
    groundMap.on("move zoom", applyMapOrientation);
    try {
        const container = groundMap.getContainer();
        if (container && !container.__gs26_follow_disable_hooks) {
            container.__gs26_follow_disable_hooks = true;
            container.addEventListener("wheel", disableFollowUserFromMapInteraction, {passive: true});
        }
    } catch (e) {
    }
    rememberMapView();
    window.__gs26_ground_map = groundMap;

    if (lastRocketLatLng) {
        rocketMarker = L.marker(lastRocketLatLng, {
            icon: makeEmojiIcon("🚀", "rocket-marker"),
            title: trackedAssetTitle(),
        }).addTo(groundMap);
        updateMarkerCounterRotation(rocketMarker);
    }

    if (lastUserLatLng) {
        userMarker = L.marker(lastUserLatLng, {
            icon: makeUserIcon(),
            title: "You",
        }).addTo(groundMap);
        updateMarkerCounterRotation(userMarker);
    }

    syncRocketGuideLine(lastRocketLatLng, lastUserLatLng);
    applyMapOrientation();
    scheduleHighResTilePrefetch();
}

function updateGroundMapMarkers(rLat, rLon, uLat, uLon) {
    const hasRocket = Number.isFinite(rLat) && Number.isFinite(rLon);
    const hasUser = Number.isFinite(uLat) && Number.isFinite(uLon);

    if (hasRocket) {
        lastRocketLatLng = [rLat, rLon];
    }

    if (hasUser) {
        lastUserLatLng = [uLat, uLon];
    }

    if (!groundMap) return;
    const L = getLeaflet();

    if (hasRocket) {
        if (!rocketMarker) {
            rocketMarker = L.marker(lastRocketLatLng, {
                icon: makeEmojiIcon("🚀", "rocket-marker"),
                title: trackedAssetTitle(),
            }).addTo(groundMap);
            updateMarkerCounterRotation(rocketMarker);
        } else {
            rocketMarker.setLatLng(lastRocketLatLng);
        }
    }

    if (hasUser) {
        if (!userMarker) {
            userMarker = L.marker(lastUserLatLng, {
                icon: makeUserIcon(),
                title: "You",
            }).addTo(groundMap);
            updateMarkerCounterRotation(userMarker);
        } else {
            userMarker.setLatLng(lastUserLatLng);
        }
        if (followUserEnabled) {
            suppressFollowDisableUntilMs = Date.now() + 750;
            groundMap.setView(lastUserLatLng, groundMap.getZoom(), {animate: false});
            correctVisualCenterOnMarker(userMarker);
        }
    }

    syncRocketGuideLine(hasRocket ? lastRocketLatLng : null, hasUser ? lastUserLatLng : null);
    applyMapOrientation();
    if (hasUser || hasRocket) {
        scheduleHighResTilePrefetch();
    }
}

// ---- keep as global script ----
(function pinGroundStation26() {
    // Put everything on ONE namespace so it’s easy to inspect/debug.
    const api = (window.GS26 = window.GS26 || {});

    // Public API you call from Rust:
    api.initGroundMap = initGroundMap;
    api.updateGroundMapMarkers = updateGroundMapMarkers;
    api.centerGroundMapOn = centerGroundMapOn;
    api.getLastUserLatLng = getLastUserLatLng;
    api.scheduleHighResTilePrefetch = scheduleHighResTilePrefetch;
    api.setGroundMapFollowUser = setGroundMapFollowUser;
    api.setGroundMapOrientationMode = setGroundMapOrientationMode;
    api.disableFollowUserFromMapInteraction = disableFollowUserFromMapInteraction;
    api.adjustGroundMapBearing = adjustGroundMapBearing;
    api.setGroundMapBearing = setGroundMapBearing;

    // Optional: expose these too (useful for debugging / permissions testing)
    api.initCompassOnce = initCompassOnce;
    api.handleOrientation = handleOrientation;

    // Pin “internal” helpers so minifiers don’t decide they’re dead:
    api.getLeaflet = getLeaflet;
    api.normalizeAngle = normalizeAngle;
    api.shortestAngleDiff = shortestAngleDiff;

    api.ensureMarkerStylesOnce = ensureMarkerStylesOnce;
    api.createNaTileLayer = createNaTileLayer;
    api.rememberMapView = rememberMapView;

    api.makeEmojiIcon = makeEmojiIcon;
    api.makeUserIcon = makeUserIcon;
    api.updateUserMarkerRotation = updateUserMarkerRotation;
    api.setGroundMapUserHeading = setGroundMapUserHeading;
    api.applyMapOrientation = applyMapOrientation;
    api.syncRocketGuideLine = syncRocketGuideLine;

    // Pin state too (lets you debug on-device):
    api.state = api.state || {};
    Object.assign(api.state, {
        NA_BOUNDS,
        MIN_ZOOM,
        DEFAULT_MAX_NATIVE_ZOOM,
        MARKER_PX,
        MARKER_ANCHOR,
        USER_FONT_PX,
        ROCKET_FONT_PX,
        ARROW_FONT_PX,
        ARROW_RADIUS,

        // live mutable state pointers (debug only)
        get groundMap() {
            return groundMap;
        },
        get rocketMarker() {
            return rocketMarker;
        },
        get userMarker() {
            return userMarker;
        },
        get lastRocketLatLng() {
            return lastRocketLatLng;
        },
        get lastUserLatLng() {
            return lastUserLatLng;
        },
        get followUserEnabled() {
            return followUserEnabled;
        },
        get orientationMode() {
            return orientationMode;
        },
        get mapBearingDeg() {
            return mapBearingDeg;
        },
        get lastMapView() {
            return lastMapView;
        },
        get userHeadingDegRaw() {
            return userHeadingDegRaw;
        },
        get userHeadingDeg() {
            return userHeadingDeg;
        },
        get compassInitialized() {
            return compassInitialized;
        },
        get tilePrefetchState() {
            return tilePrefetchState;
        },
    });

    window.initGroundMap = api.initGroundMap;
    window.updateGroundMapMarkers = api.updateGroundMapMarkers;
    window.centerGroundMapOn = api.centerGroundMapOn;
    window.getLastUserLatLng = api.getLastUserLatLng;
    window.initCompassOnce = api.initCompassOnce;
    window.setGroundMapUserHeading = api.setGroundMapUserHeading;
    window.setGroundMapFollowUser = api.setGroundMapFollowUser;
    window.setGroundMapOrientationMode = api.setGroundMapOrientationMode;
    window.adjustGroundMapBearing = api.adjustGroundMapBearing;
    window.setGroundMapBearing = api.setGroundMapBearing;
    window.scheduleHighResTilePrefetch = api.scheduleHighResTilePrefetch;

    // “Loaded” flag
    window.__gs26_ground_station_loaded = true;
    window.__gs26_ground_map_cache_state = {...tilePrefetchState};
    window.__gs26_ground_map_cache_ready = false;
    console.log("[GS26] ground_station.js loaded; keys:", Object.keys(api));
})();
