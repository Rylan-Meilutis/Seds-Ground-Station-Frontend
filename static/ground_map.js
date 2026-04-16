//
// GroundStation26 map runtime
// Map engine: MapLibre GL JS
//

let groundMap = null;
let userMarkerDisplayedLatLng = null;
let userMarkerAnimationFrame = null;
let userMarkerAnimation = null;
let followCameraFrame = null;
let pendingFollowCameraLatLng = null;
let currentTilesUrl = null;
let configuredMaxNativeZoom = null;
let configuredMaxDisplayZoom = null;
let currentMaxNativeZoom = null;
let currentMaxZoom = null;
let lastRocketLatLng = null;
let lastUserLatLng = null;
let lastMapView = null;
let tileCacheSweepTimer = null;
let tilePrefetchTimer = null;
let tileZoomDiscoveryTimer = null;
let tileZoomDiscoveryKey = "";
let tileZoomDiscoveryRunId = 0;
let currentPrefetchKey = null;
let tilePrefetchRunId = 0;
let prefetchSuppressedUntilMs = 0;
let mapInitStartedAtMs = 0;
let lastPersistedMapStateAtMs = 0;
let followUserEnabled = false;
let orientationMode = "north";
let mapBearingDeg = 0;
let suppressFollowCameraUntilMs = 0;
let suppressFollowDisableUntilMs = 0;
let followEnableGuardUntilMs = 0;
let internalCameraUpdateUntilMs = 0;
let userHeadingDegRaw = null;
let userHeadingDeg = null;
let nativeHeadingDeg = null;
let deviceHeadingDeg = null;
let compassInitialized = false;
let maplibreProtocolInstalled = false;
let mapReady = false;
let trackedAssetLabel = "Tracked Asset";
let mapNavigationControl = null;
let mapCenterControl = null;
let mapNorthControl = null;
const TILE_PROTOCOL = "gs26map";

let tilePrefetchState = {
    key: "",
    state: "idle",
    pending: 0,
    completed: 0,
    failed: 0,
    lastStartedAt: 0,
    lastCompletedAt: 0,
};

const MIN_ZOOM = 0;
const DEFAULT_MAX_NATIVE_ZOOM = 12;
const DEFAULT_MAX_OVERZOOM_DELTA = 1;
const HIGH_RES_PREFETCH_RADIUS_M = 1609.344;
const HIGH_RES_PREFETCH_MIN_ZOOM_DELTA = 1;
const HIGH_RES_PREFETCH_MAX_TILES = 96;
const HIGH_RES_PREFETCH_CONCURRENCY = 1;
const HIGH_RES_PREFETCH_STARTUP_DELAY_MS = 5000;
const HIGH_RES_PREFETCH_IDLE_DELAY_MS = 2500;
const CACHE_SWEEP_DELAY_MS = 15000;
const USER_MARKER_SMOOTH_MIN_MS = 80;
const USER_MARKER_SMOOTH_MAX_MS = 220;
const USER_MARKER_SMOOTH_SNAP_DISTANCE_M = 20.0;
const USER_MARKER_SMOOTH_SKIP_M = 0.35;
const TILE_SOURCE_ID = "gs26-raster-source";
const TILE_LAYER_ID = "gs26-raster-layer";
const GUIDE_SOURCE_ID = "gs26-guide-source";
const GUIDE_LAYER_ID = "gs26-guide-layer";
const USER_SOURCE_ID = "gs26-user-source";
const USER_LAYER_ID = "gs26-user-layer";
const ROCKET_SOURCE_ID = "gs26-rocket-source";
const ROCKET_LAYER_ID = "gs26-rocket-layer";
const USER_HEADING_SOURCE_ID = "gs26-user-heading-source";
const USER_HEADING_LAYER_ID = "gs26-user-heading-layer";
const MAP_STATE_STORAGE_KEY = "gs26_ground_map_state_v3";
const USER_ICON_IMAGE_ID = "gs26-user-icon";
const ROCKET_ICON_IMAGE_ID = "gs26-rocket-icon";
const USER_HEADING_IMAGE_ID = "gs26-user-heading-icon";

const NA_BOUNDS = {
    lonMin: -170.0,
    latMin: 5.0,
    lonMax: -50.0,
    latMax: 83.0,
};

const TWO_TOUCH_ROTATE_THRESHOLD_DEG = 8;

function getMapLibre() {
    if (!window.maplibregl || typeof window.maplibregl.Map !== "function") {
        throw new Error("MapLibre GL JS is not available on window.maplibregl");
    }
    return window.maplibregl;
}

function isAndroidPlatform() {
    if (typeof navigator === "undefined") return false;
    const userAgent = navigator.userAgent || navigator.platform || "";
    return /Android/i.test(userAgent);
}

function normalizeAngle(deg) {
    let value = Number(deg) || 0;
    value %= 360;
    if (value < 0) value += 360;
    return value;
}

function markInternalCameraUpdate(durationMs) {
    internalCameraUpdateUntilMs = Date.now() + Math.max(0, Number(durationMs) || 0);
}

function isInternalCameraUpdate() {
    return Date.now() < internalCameraUpdateUntilMs;
}

function shortestAngleDiff(a, b) {
    let diff = normalizeAngle(b) - normalizeAngle(a);
    if (diff > 180) diff -= 360;
    if (diff < -180) diff += 360;
    return diff;
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

function tileCacheSupported() {
    return typeof window !== "undefined"
        && typeof window.caches !== "undefined"
        && typeof window.fetch === "function";
}

function tileCacheAllowedForUrl(url) {
    return tileCacheSupported() && /^https?:/i.test(String(url || ""));
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

function idleDelay(callback, timeoutMs) {
    if (typeof window !== "undefined" && typeof window.requestIdleCallback === "function") {
        return {kind: "idle", handle: window.requestIdleCallback(callback, {timeout: timeoutMs})};
    }
    return {kind: "timeout", handle: setTimeout(callback, timeoutMs)};
}

function cancelIdleDelay(timer) {
    if (!timer) return;
    if (timer.kind === "idle" && typeof window !== "undefined" && typeof window.cancelIdleCallback === "function") {
        try {
            window.cancelIdleCallback(timer.handle);
            return;
        } catch (e) {
        }
    }
    clearTimeout(timer.handle);
}

function suppressHighResPrefetch(ms) {
    prefetchSuppressedUntilMs = Math.max(
        prefetchSuppressedUntilMs,
        Date.now() + Math.max(0, Number(ms) || 0)
    );
    if (tilePrefetchTimer) {
        cancelIdleDelay(tilePrefetchTimer);
        tilePrefetchTimer = null;
    }
    tilePrefetchRunId += 1;
}

function tileCacheName(tilesUrl) {
    return `gs26-tiles-v1:${String(tilesUrl || "")
        .replace(/[^a-z0-9]+/gi, "_")
        .replace(/^_+|_+$/g, "")
        .toLowerCase() || "default"}`;
}

function resolveTileUrl(z, x, y) {
    if (!currentTilesUrl) return "";
    return String(currentTilesUrl)
        .replace("{z}", String(z))
        .replace("{x}", String(x))
        .replace("{y}", String(y));
}

function tileProtocolTemplate() {
    return `${TILE_PROTOCOL}://tiles/{z}/{x}/{y}.jpg`;
}

function shouldUseNativeTileTemplate(tilesUrl) {
    const url = String(tilesUrl || "");
    return /^gs26:\/\//i.test(url)
        || /^https:\/\/gs26\.local\//i.test(url)
        || /^http:\/\/gs26\.localhost\//i.test(url);
}

function tilesUseNativeProxy() {
    return shouldUseNativeTileTemplate(currentTilesUrl);
}

function tilesUseCustomSchemeProxy() {
    return /^gs26:\/\//i.test(String(currentTilesUrl || ""));
}

function parseTileProtocolRequest(url) {
    const match = String(url || "").match(/^gs26map:\/\/tiles\/(\d+)\/(\d+)\/(\d+)\.jpg(?:\?.*)?$/i);
    if (!match) return null;
    return {
        z: Number(match[1]),
        x: Number(match[2]),
        y: Number(match[3]),
    };
}

function ensureMapProtocolOnce() {
    if (maplibreProtocolInstalled) return;
    const maplibre = getMapLibre();
    if (typeof maplibre.addProtocol !== "function") return;

    maplibre.addProtocol(TILE_PROTOCOL, async (params) => {
        const coords = parseTileProtocolRequest(params && params.url);
        if (!coords) {
            throw new Error(`invalid tile protocol url: ${params && params.url ? params.url : ""}`);
        }
        const url = resolveTileUrl(coords.z, coords.x, coords.y);
        if (!url) {
            throw new Error("tile url missing");
        }
        const cacheName = tileCacheName(currentTilesUrl);
        try {
            const data = await fetchAndCacheTileArrayBuffer(cacheName, url);
            return {data};
        } catch (primaryError) {
            const cached = await readCachedTileArrayBuffer(cacheName, url);
            if (cached) {
                return {data: cached};
            }
            throw primaryError;
        }
    });
    maplibreProtocolInstalled = true;
}

async function readCachedTileArrayBuffer(cacheName, url) {
    if (!tileCacheAllowedForUrl(url) || !url) return null;
    try {
        const cache = await caches.open(cacheName);
        const cached = await cache.match(url, {ignoreVary: true});
        if (!cached || !cached.ok) return null;
        return await cached.arrayBuffer();
    } catch (e) {
        console.warn("[GS26 map] cache read failed", url, e);
        return null;
    }
}

async function fetchAndCacheTileArrayBuffer(cacheName, url) {
    if (!url) throw new Error("tile url missing");

    if (!tileCacheAllowedForUrl(url)) {
        const response = await fetch(url);
        if (!response.ok) throw new Error(`tile fetch failed: ${response.status}`);
        return await response.arrayBuffer();
    }

    const cache = await caches.open(cacheName);
    const cached = await cache.match(url, {ignoreVary: true});
    if (cached && cached.ok) {
        return await cached.arrayBuffer();
    }

    const response = await fetch(url);
    if (!response.ok) throw new Error(`tile fetch failed: ${response.status}`);
    Promise.resolve().then(async () => {
        try {
            await cache.put(url, response.clone());
        } catch (e) {
            console.warn("[GS26 map] cache put failed", url, e);
        }
    });
    return await response.arrayBuffer();
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

function metersPerDegreeLat() {
    return 111_320.0;
}

function metersPerDegreeLon(lat) {
    const cosLat = Math.cos((lat * Math.PI) / 180.0);
    return Math.max(1.0, 111_320.0 * Math.max(0.01, Math.abs(cosLat)));
}

function distanceMetersBetween(a, b) {
    if (!Array.isArray(a) || !Array.isArray(b)) return Infinity;
    const lat1 = Number(a[0]);
    const lon1 = Number(a[1]);
    const lat2 = Number(b[0]);
    const lon2 = Number(b[1]);
    if (![lat1, lon1, lat2, lon2].every(Number.isFinite)) return Infinity;
    const r = 6371000.0;
    const p1 = lat1 * Math.PI / 180.0;
    const p2 = lat2 * Math.PI / 180.0;
    const dLat = (lat2 - lat1) * Math.PI / 180.0;
    const dLon = (lon2 - lon1) * Math.PI / 180.0;
    const h = Math.sin(dLat / 2.0) ** 2
        + Math.cos(p1) * Math.cos(p2) * Math.sin(dLon / 2.0) ** 2;
    return 2.0 * r * Math.atan2(Math.sqrt(h), Math.sqrt(Math.max(0.0, 1.0 - h)));
}

function emptyFeatureCollection() {
    return {
        type: "FeatureCollection",
        features: [],
    };
}

function pointFeatureCollection(latLng) {
    if (!Array.isArray(latLng)) return emptyFeatureCollection();
    return {
        type: "FeatureCollection",
        features: [{
            type: "Feature",
            geometry: {
                type: "Point",
                coordinates: [latLng[1], latLng[0]],
            },
            properties: {},
        }],
    };
}

function headingFeatureCollection(latLng, headingDeg) {
    if (!Array.isArray(latLng)) return emptyFeatureCollection();
    const resolvedHeadingDeg = Number.isFinite(headingDeg) ? normalizeAngle(headingDeg) : 0;
    return {
        type: "FeatureCollection",
        features: [{
            type: "Feature",
            geometry: {
                type: "Point",
                coordinates: [latLng[1], latLng[0]],
            },
            properties: {bearing: resolvedHeadingDeg},
        }],
    };
}

function createMarkerCanvas(size, draw) {
    const canvas = document.createElement("canvas");
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext("2d");
    draw(ctx, size);
    return ctx.getImageData(0, 0, size, size);
}

function createUserIconImage() {
    return createMarkerCanvas(48, (ctx, size) => {
        ctx.clearRect(0, 0, size, size);
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.font = "20px system-ui, -apple-system, BlinkMacSystemFont, sans-serif";
        ctx.fillText("🧍", size * 0.5, size * 0.53);
    });
}

function createRocketIconImage() {
    return createMarkerCanvas(48, (ctx, size) => {
        ctx.clearRect(0, 0, size, size);
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.font = "32px system-ui, -apple-system, BlinkMacSystemFont, sans-serif";
        ctx.fillText("🚀", size * 0.5, size * 0.52);
    });
}

function createHeadingArrowImage() {
    return createMarkerCanvas(32, (ctx, size) => {
        ctx.clearRect(0, 0, size, size);
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.font = "18px system-ui, -apple-system, BlinkMacSystemFont, sans-serif";
        ctx.fillStyle = "#f8fafc";
        ctx.strokeStyle = "#111827";
        ctx.lineWidth = 2;
        ctx.strokeText("▲", size * 0.5, size * 0.53);
        ctx.fillText("▲", size * 0.5, size * 0.53);
    });
}

function ensureMapMarkerImages() {
    if (!groundMap || typeof groundMap.hasImage !== "function" || typeof groundMap.addImage !== "function") return;
    if (!groundMap.hasImage(USER_ICON_IMAGE_ID)) {
        groundMap.addImage(USER_ICON_IMAGE_ID, createUserIconImage(), {pixelRatio: 1});
    }
    if (!groundMap.hasImage(ROCKET_ICON_IMAGE_ID)) {
        groundMap.addImage(ROCKET_ICON_IMAGE_ID, createRocketIconImage(), {pixelRatio: 1});
    }
    if (!groundMap.hasImage(USER_HEADING_IMAGE_ID)) {
        groundMap.addImage(USER_HEADING_IMAGE_ID, createHeadingArrowImage(), {pixelRatio: 1});
    }
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

function tileCoordsForBounds(bounds, zoom) {
    if (!bounds) return [];
    const north = bounds.getNorth();
    const south = bounds.getSouth();
    const east = bounds.getEast();
    const west = bounds.getWest();
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
    const top = Math.max(MIN_ZOOM, Math.floor(Number(maxNativeZoom) || DEFAULT_MAX_NATIVE_ZOOM));
    const min = Math.max(MIN_ZOOM, top - HIGH_RES_PREFETCH_MIN_ZOOM_DELTA);
    const zooms = [];
    for (let z = top; z >= min; z--) {
        zooms.push(z);
    }
    return zooms;
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

function clampMaxNativeZoom(value) {
    if (!Number.isFinite(value)) return DEFAULT_MAX_NATIVE_ZOOM;
    return Math.max(MIN_ZOOM, Math.floor(value));
}

function clampMaxDisplayZoom(value, maxNativeZoom) {
    const nativeZoom = clampMaxNativeZoom(maxNativeZoom);
    if (!Number.isFinite(value)) return nativeZoom + DEFAULT_MAX_OVERZOOM_DELTA;
    return Math.max(nativeZoom + DEFAULT_MAX_OVERZOOM_DELTA, Math.floor(value));
}

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

function persistMapStateSoon() {
    const now = Date.now();
    if (now - lastPersistedMapStateAtMs < 250) return;
    lastPersistedMapStateAtMs = now;
    persistMapState();
}

loadPersistedMapState();

function rememberMapView() {
    if (!groundMap) return;
    const center = groundMap.getCenter();
    lastMapView = {
        lat: center.lat,
        lon: center.lng,
        zoom: groundMap.getZoom(),
    };
    mapBearingDeg = normalizeAngle(groundMap.getBearing());
    persistMapStateSoon();
}

function effectiveMaxNativeZoomFor(configMaxNativeZoom) {
    return clampMaxNativeZoom(configMaxNativeZoom);
}

function scheduleTileZoomDiscovery() {
    return;
}

function buildHighResPrefetchPlan() {
    if (!groundMap || !currentTilesUrl) {
        return {key: "", coords: []};
    }

    const zooms = prefetchZoomLevels(currentMaxNativeZoom);
    const coords = [];
    const seen = new Set();
    const userLat = Array.isArray(lastUserLatLng) ? lastUserLatLng[0] : NaN;
    const userLon = Array.isArray(lastUserLatLng) ? lastUserLatLng[1] : NaN;
    const rocketLat = Array.isArray(lastRocketLatLng) ? lastRocketLatLng[0] : NaN;
    const rocketLon = Array.isArray(lastRocketLatLng) ? lastRocketLatLng[1] : NaN;
    const bounds = groundMap.getBounds ? groundMap.getBounds() : null;
    const viewportKey = bounds
        ? [
            bounds.getNorth().toFixed(4),
            bounds.getSouth().toFixed(4),
            bounds.getEast().toFixed(4),
            bounds.getWest().toFixed(4),
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

        if (bounds && coords.length < HIGH_RES_PREFETCH_MAX_TILES) {
            appendUniqueCoords(
                coords,
                seen,
                tileCoordsForBounds(bounds, zoom),
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
    const tilesUrl = currentTilesUrl;
    if (!tilesUrl) return;

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
            if (runId !== tilePrefetchRunId || currentTilesUrl !== tilesUrl) return;
            const index = nextIndex++;
            if (index >= total) return;
            const coord = plan.coords[index];
            try {
                await fetchAndCacheTileArrayBuffer(
                    cacheName,
                    resolveTileUrl(coord.z, coord.x, coord.y)
                );
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
    if (!tileCacheAllowedForUrl(tilesUrl)) return;
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
    }, CACHE_SWEEP_DELAY_MS);
}

function scheduleHighResTilePrefetch() {
    if (!groundMap || !currentTilesUrl) return;
    if (tilesUseCustomSchemeProxy() || !tileCacheAllowedForUrl(currentTilesUrl)) {
        currentPrefetchKey = "";
        setTilePrefetchState({
            key: "",
            state: "idle",
            pending: 0,
            completed: 0,
            failed: 0,
        });
        return;
    }
    const plan = buildHighResPrefetchPlan();
    const key = plan.key;
    if (!key) return;
    if (currentPrefetchKey === key) return;
    currentPrefetchKey = key;

    if (tilePrefetchTimer) cancelIdleDelay(tilePrefetchTimer);
    const runId = ++tilePrefetchRunId;
    setTilePrefetchState({
        key,
        state: "queued",
        pending: plan.coords.length,
        completed: 0,
        failed: 0,
    });
    const now = Date.now();
    const sinceInitMs = mapInitStartedAtMs > 0 ? Math.max(0, now - mapInitStartedAtMs) : HIGH_RES_PREFETCH_STARTUP_DELAY_MS;
    const startupDelayMs = Math.max(HIGH_RES_PREFETCH_IDLE_DELAY_MS, HIGH_RES_PREFETCH_STARTUP_DELAY_MS - sinceInitMs);
    const suppressionDelayMs = Math.max(0, prefetchSuppressedUntilMs - now);
    const delayMs = Math.max(startupDelayMs, suppressionDelayMs);
    tilePrefetchTimer = idleDelay(async () => {
        tilePrefetchTimer = null;
        if (Date.now() < prefetchSuppressedUntilMs) {
            scheduleHighResTilePrefetch();
            return;
        }
        await runHighResTilePrefetch(runId, key);
    }, delayMs);
}

function ensureMarkerStylesOnce() {
    if (document.getElementById("gs26-marker-styles")) return;

    const style = document.createElement("style");
    style.id = "gs26-marker-styles";
    style.textContent = `
    .gs26-map-center-control {
      position: relative;
    }

    .gs26-map-center-control-icon {
      position: absolute;
      left: 50%;
      top: 50%;
      width: 17px;
      height: 17px;
      transform: translate(-50%, -50%);
      border: 2px solid #0f172a;
      border-radius: 999px;
      box-sizing: border-box;
      transition: background-color 140ms ease, border-color 140ms ease, transform 140ms ease;
    }

    .gs26-map-center-control-icon::before,
    .gs26-map-center-control-icon::after {
      content: "";
      position: absolute;
      background: #0f172a;
      left: 50%;
      top: 50%;
      transform: translate(-50%, -50%);
    }

    .gs26-map-center-control-icon::before {
      width: 2px;
      height: 11px;
    }

    .gs26-map-center-control-icon::after {
      width: 11px;
      height: 2px;
    }

    .gs26-map-center-control[data-mode="follow"] .gs26-map-center-control-icon {
      background: rgba(15, 23, 42, 0.16);
    }

    .gs26-map-center-control[data-mode="user-up"] .gs26-map-center-control-icon {
      width: 0;
      height: 0;
      border-left: 8px solid transparent;
      border-right: 8px solid transparent;
      border-bottom: 16px solid #0f172a;
      border-top: 0;
      border-radius: 0;
      background: transparent;
      transform: translate(-50%, calc(-50% - 1px));
    }

    .gs26-map-center-control[data-mode="user-up"] .gs26-map-center-control-icon::before,
    .gs26-map-center-control[data-mode="user-up"] .gs26-map-center-control-icon::after {
      display: none;
    }

    .gs26-map-north-control {
      position: relative;
    }

    .gs26-map-north-control[hidden] {
      display: none !important;
    }

    .gs26-map-north-control-icon {
      position: relative;
      width: 18px;
      height: 18px;
      display: block;
      transform-origin: 50% 50%;
      transition: transform 140ms ease;
    }

    .gs26-map-north-control-icon::before {
      content: "";
      position: absolute;
      left: 50%;
      top: 1px;
      transform: translateX(-50%);
      width: 0;
      height: 0;
      border-left: 6px solid transparent;
      border-right: 6px solid transparent;
      border-bottom: 10px solid #dc2626;
    }

    .gs26-map-north-control-icon::after {
      content: "N";
      position: absolute;
      left: 50%;
      bottom: -1px;
      transform: translateX(-50%);
      font-size: 8px;
      font-weight: 800;
      line-height: 1;
      color: #0f172a;
      font-family: system-ui, -apple-system, BlinkMacSystemFont, sans-serif;
    }
  `;
    document.head.appendChild(style);
}

function touchAngle(touches) {
    const dx = touches[1].clientX - touches[0].clientX;
    const dy = touches[1].clientY - touches[0].clientY;
    return normalizeAngle(Math.atan2(dy, dx) * 180.0 / Math.PI);
}

function touchDistance(touches) {
    const dx = touches[1].clientX - touches[0].clientX;
    const dy = touches[1].clientY - touches[0].clientY;
    return Math.hypot(dx, dy);
}

function touchMidpoint(touches) {
    return {
        x: (touches[0].clientX + touches[1].clientX) / 2,
        y: (touches[0].clientY + touches[1].clientY) / 2,
    };
}

function updateUserMarkerRotation() {
    syncUserHeadingIndicator();
}

function centerControlMode() {
    if (followUserEnabled && orientationMode === "user") return "user-up";
    if (followUserEnabled) return "follow";
    return "idle";
}

function updateCenterControlAppearance() {
    if (!mapCenterControl || !mapCenterControl._button) return;
    const mode = centerControlMode();
    mapCenterControl._button.setAttribute("data-mode", mode);
    mapCenterControl._button.title =
        mode === "user-up"
            ? "User Up Enabled"
            : mode === "follow"
                ? "Auto Center Enabled"
                : "Center On Me";
    mapCenterControl._button.setAttribute(
        "aria-label",
        mode === "user-up"
            ? "User Up Enabled"
            : mode === "follow"
                ? "Auto Center Enabled"
                : "Center On Me"
    );
    updateNorthControlAppearance();
}

function shouldShowNorthControl() {
    return Math.abs(shortestAngleDiff(mapBearingDeg, 0)) >= 1.0;
}

function updateNorthControlAppearance() {
    if (!mapNorthControl || !mapNorthControl._button || !mapNorthControl._icon) return;
    const show = shouldShowNorthControl();
    mapNorthControl._button.hidden = !show;
    mapNorthControl._button.title = "Reset North Up";
    mapNorthControl._button.setAttribute("aria-label", "Reset North Up");
    mapNorthControl._icon.style.transform = `rotate(${-mapBearingDeg}deg)`;
}

function syncRocketGuideLine(rocketLatLng, userLatLng) {
    if (!groundMap || !mapReady) return;
    const source = groundMap.getSource(GUIDE_SOURCE_ID);
    if (!source) return;

    if (!rocketLatLng || !userLatLng) {
        source.setData({
            type: "FeatureCollection",
            features: [],
        });
        return;
    }

    source.setData({
        type: "FeatureCollection",
        features: [{
            type: "Feature",
            geometry: {
                type: "LineString",
                coordinates: [
                    [userLatLng[1], userLatLng[0]],
                    [rocketLatLng[1], rocketLatLng[0]],
                ],
            },
            properties: {},
        }],
    });
}

function syncPointSource(sourceId, latLng) {
    if (!groundMap || !mapReady) return;
    const source = groundMap.getSource(sourceId);
    if (!source) return;
    source.setData(pointFeatureCollection(latLng));
}

function syncUserHeadingIndicator() {
    if (!groundMap || !mapReady) return;
    const source = groundMap.getSource(USER_HEADING_SOURCE_ID);
    if (!source) return;
    const latLng = userMarkerDisplayedLatLng || lastUserLatLng;
    source.setData(headingFeatureCollection(latLng, userHeadingDeg));
}

function scheduleFollowCameraUpdate(latLng) {
    if (!groundMap || !Array.isArray(latLng)) return;
    if (Date.now() < suppressFollowCameraUntilMs) return;
    pendingFollowCameraLatLng = [latLng[0], latLng[1]];
    if (followCameraFrame != null) return;
    followCameraFrame = requestAnimationFrame(() => {
        followCameraFrame = null;
        const target = pendingFollowCameraLatLng;
        pendingFollowCameraLatLng = null;
        if (!groundMap || !Array.isArray(target)) return;
        markInternalCameraUpdate(120);
        groundMap.jumpTo({
            center: [target[1], target[0]],
            bearing: mapBearingDeg,
        });
        rememberMapView();
    });
}

function setUserMarkerVisualLatLng(latLng) {
    if (!Array.isArray(latLng)) return;
    userMarkerDisplayedLatLng = [latLng[0], latLng[1]];
    syncPointSource(USER_SOURCE_ID, userMarkerDisplayedLatLng);
    syncUserHeadingIndicator();
    syncRocketGuideLine(lastRocketLatLng, userMarkerDisplayedLatLng);
    if (followUserEnabled && groundMap) {
        scheduleFollowCameraUpdate(latLng);
    }
}

function currentUserMarkerVisualLatLng() {
    const displayed = Array.isArray(userMarkerDisplayedLatLng) ? userMarkerDisplayedLatLng : null;
    const anim = userMarkerAnimation;
    if (!anim) return displayed;

    const now = performance.now();
    const t = Math.max(0.0, Math.min(1.0, (now - anim.startedAt) / anim.durationMs));
    if (t >= 1.0) return anim.target;
    const eased = 1.0 - Math.pow(1.0 - t, 3.0);
    return [
        anim.from[0] + (anim.target[0] - anim.from[0]) * eased,
        anim.from[1] + (anim.target[1] - anim.from[1]) * eased,
    ];
}

function cancelUserMarkerAnimation() {
    if (followCameraFrame != null) {
        try {
            cancelAnimationFrame(followCameraFrame);
        } catch (e) {
        }
    }
    followCameraFrame = null;
    pendingFollowCameraLatLng = null;
    if (userMarkerAnimationFrame != null) {
        try {
            cancelAnimationFrame(userMarkerAnimationFrame);
        } catch (e) {
        }
    }
    userMarkerAnimationFrame = null;
    userMarkerAnimation = null;
}

function animateUserMarkerTo(targetLatLng) {
    if (!Array.isArray(targetLatLng)) return;
    const target = [targetLatLng[0], targetLatLng[1]];
    const from = currentUserMarkerVisualLatLng() || target;
    const distanceM = distanceMetersBetween(from, target);

    if (
        !Number.isFinite(distanceM)
        || distanceM <= USER_MARKER_SMOOTH_SKIP_M
        || distanceM >= USER_MARKER_SMOOTH_SNAP_DISTANCE_M
    ) {
        cancelUserMarkerAnimation();
        setUserMarkerVisualLatLng(target);
        return;
    }

    setUserMarkerVisualLatLng(from);
    const durationMs = Math.max(
        USER_MARKER_SMOOTH_MIN_MS,
        Math.min(USER_MARKER_SMOOTH_MAX_MS, 90.0 + distanceM * 10.0)
    );
    userMarkerAnimation = {
        from,
        target,
        startedAt: performance.now(),
        durationMs,
    };

    if (userMarkerAnimationFrame != null) return;

    const step = () => {
        const anim = userMarkerAnimation;
        if (!anim) {
            userMarkerAnimationFrame = null;
            return;
        }
        const now = performance.now();
        const t = Math.max(0.0, Math.min(1.0, (now - anim.startedAt) / anim.durationMs));
        const eased = 1.0 - Math.pow(1.0 - t, 3.0);
        const next = [
            anim.from[0] + (anim.target[0] - anim.from[0]) * eased,
            anim.from[1] + (anim.target[1] - anim.from[1]) * eased,
        ];
        if (t >= 1.0) {
            setUserMarkerVisualLatLng(anim.target);
            userMarkerAnimation = null;
            userMarkerAnimationFrame = null;
            return;
        }
        setUserMarkerVisualLatLng(next);
        userMarkerAnimationFrame = requestAnimationFrame(step);
    };
    userMarkerAnimationFrame = requestAnimationFrame(step);
}

function fusedHeadingTarget() {
    const hasNative = Number.isFinite(nativeHeadingDeg);
    const hasDevice = Number.isFinite(deviceHeadingDeg);
    if (hasNative) return normalizeAngle(nativeHeadingDeg);
    if (hasDevice) return normalizeAngle(deviceHeadingDeg);
    return null;
}

function applyMapOrientation() {
    if (!groundMap) return;
    const targetBearing = normalizeAngle(mapBearingDeg);
    const currentBearing = normalizeAngle(groundMap.getBearing());
    if (Math.abs(shortestAngleDiff(currentBearing, targetBearing)) > 0.05) {
        markInternalCameraUpdate(250);
        groundMap.jumpTo({bearing: targetBearing});
    }
    updateUserMarkerRotation();
    rememberMapView();
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

    if (followUserEnabled && orientationMode === "user") {
        mapBearingDeg = normalizeAngle(userHeadingDeg);
    }
    updateUserMarkerRotation();
    applyMapOrientation();
}

function setGroundMapUserHeading(deg) {
    if (!Number.isFinite(deg)) return;
    nativeHeadingDeg = normalizeAngle(deg);
    applyFusedHeading();
}

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
        const key = "gs26_compass_permission_v1";
        let saved = "";
        try {
            saved = window.localStorage ? (window.localStorage.getItem(key) || "") : "";
        } catch (e) {
        }

        if (saved === "granted") {
            window.addEventListener("deviceorientation", handleOrientation);
            return;
        }
        if (saved === "denied") return;

        Dev.requestPermission()
            .then((value) => {
                try {
                    if (window.localStorage) window.localStorage.setItem(key, value || "denied");
                } catch (e) {
                }
                if (value === "granted") {
                    window.addEventListener("deviceorientation", handleOrientation);
                }
            })
            .catch(() => {
                try {
                    if (window.localStorage) window.localStorage.setItem(key, "denied");
                } catch (e) {
                }
            });
    } else {
        window.addEventListener("deviceorientation", handleOrientation);
    }
}

function applyPendingCenterIfPossible() {
    if (!groundMap) return;
    try {
        const lat = Number(window.__gs26_pending_center_lat);
        const lon = Number(window.__gs26_pending_center_lon);
        if (!Number.isFinite(lat) || !Number.isFinite(lon)) return;
        window.__gs26_pending_center_lat = NaN;
        window.__gs26_pending_center_lon = NaN;
        markInternalCameraUpdate(250);
        groundMap.jumpTo({center: [lon, lat], bearing: mapBearingDeg});
        rememberMapView();
    } catch (e) {
    }
}

function applyFollowUserIfPossible() {
    if (!groundMap || !followUserEnabled || !lastUserLatLng) return;
    if (Date.now() < suppressFollowCameraUntilMs) return;
    markInternalCameraUpdate(250);
    groundMap.jumpTo({
        center: [lastUserLatLng[1], lastUserLatLng[0]],
        bearing: mapBearingDeg,
    });
    rememberMapView();
    scheduleHighResTilePrefetch();
}

function unlockMapInteraction(options) {
    const force = !!(options && options.force);
    const dropFollow = options && Object.prototype.hasOwnProperty.call(options, "dropFollow")
        ? !!options.dropFollow
        : true;
    const dropOrientation = !!(options && options.dropOrientation);
    let changed = false;
    if (dropFollow && followUserEnabled) {
        followUserEnabled = false;
        followEnableGuardUntilMs = 0;
        changed = true;
    }
    if (dropOrientation && orientationMode === "user") {
        orientationMode = "manual";
        changed = true;
    }
    if (!changed) return;

    suppressFollowDisableUntilMs = 0;
    persistMapState();
    try {
        window.__gs26_follow_user_enabled = followUserEnabled ? "true" : "false";
        window.__gs26_follow_user_enable_guard_until = followEnableGuardUntilMs;
        window.__gs26_map_orientation_mode = orientationMode;
        if (!followUserEnabled) {
            window.dispatchEvent(new CustomEvent("gs26-follow-user-changed", {
                detail: {enabled: false},
            }));
        }
    } catch (e) {
    }
    updateCenterControlAppearance();
}

function disableFollowUserFromMapInteraction() {
    unlockMapInteraction({force: false, dropFollow: true, dropOrientation: true});
}

function syncRequestedMapControlState() {
    try {
        const enabled = String(window.__gs26_follow_user_enabled ?? "false") === "true";
        const guard = Number(window.__gs26_follow_user_enable_guard_until || 0);
        followUserEnabled = enabled || (Number.isFinite(guard) && guard > Date.now());

        const requestedMode = String(window.__gs26_map_orientation_mode || "");
        if (requestedMode === "user" || requestedMode === "north" || requestedMode === "manual") {
            orientationMode = requestedMode;
        }

        const requestedBearing = Number(window.__gs26_map_bearing_deg);
        if (Number.isFinite(requestedBearing)) {
            mapBearingDeg = normalizeAngle(requestedBearing);
        }
    } catch (e) {
    }
    updateCenterControlAppearance();
}

function setGroundMapOrientationMode(mode) {
    orientationMode = mode === "user" ? "user" : (mode === "manual" ? "manual" : "north");
    if (orientationMode === "north") {
        mapBearingDeg = 0;
    } else if (orientationMode === "user" && followUserEnabled && Number.isFinite(userHeadingDeg)) {
        mapBearingDeg = normalizeAngle(userHeadingDeg);
    }
    try {
        window.__gs26_map_orientation_mode = orientationMode;
        window.__gs26_map_bearing_deg = mapBearingDeg;
    } catch (e) {
    }
    persistMapState();
    applyMapOrientation();
    updateCenterControlAppearance();
}

function enterManualOrientationMode() {
    if (orientationMode !== "manual") {
        orientationMode = "manual";
        try {
            window.__gs26_map_orientation_mode = "manual";
        } catch (e) {
        }
    }
    updateCenterControlAppearance();
}

function adjustGroundMapBearing(deltaDeg) {
    const delta = Number(deltaDeg);
    if (!Number.isFinite(delta)) return;
    orientationMode = "manual";
    mapBearingDeg = normalizeAngle(mapBearingDeg + delta);
    try {
        window.__gs26_map_orientation_mode = orientationMode;
        window.__gs26_map_bearing_deg = mapBearingDeg;
    } catch (e) {
    }
    persistMapState();
    applyMapOrientation();
}

function setGroundMapBearing(deg) {
    if (!Number.isFinite(deg)) return;
    orientationMode = "manual";
    mapBearingDeg = normalizeAngle(deg);
    try {
        window.__gs26_map_orientation_mode = orientationMode;
        window.__gs26_map_bearing_deg = mapBearingDeg;
    } catch (e) {
    }
    persistMapState();
    applyMapOrientation();
}

function setGroundMapFollowUser(enabled) {
    followUserEnabled = enabled === true;
    followEnableGuardUntilMs = followUserEnabled ? Date.now() + 5000 : 0;
    try {
        window.__gs26_follow_user_enabled = followUserEnabled ? "true" : "false";
        window.__gs26_follow_user_enable_guard_until = followEnableGuardUntilMs;
        window.dispatchEvent(new CustomEvent("gs26-follow-user-changed", {
            detail: {enabled: followUserEnabled},
        }));
    } catch (e) {
    }
    if (followUserEnabled && orientationMode === "user" && Number.isFinite(userHeadingDeg)) {
        mapBearingDeg = normalizeAngle(userHeadingDeg);
    }
    if (followUserEnabled) {
        applyFollowUserIfPossible();
    }
    persistMapState();
    applyMapOrientation();
    updateCenterControlAppearance();
}

function centerOnUserNow() {
    if (!groundMap || !lastUserLatLng) return false;
    markInternalCameraUpdate(250);
    groundMap.jumpTo({
        center: [lastUserLatLng[1], lastUserLatLng[0]],
        bearing: mapBearingDeg,
    });
    rememberMapView();
    scheduleHighResTilePrefetch();
    return true;
}

function activateLocateControl() {
    if (!followUserEnabled) {
        setGroundMapFollowUser(true);
        setGroundMapOrientationMode("north");
        centerOnUserNow();
        return;
    }

    if (orientationMode !== "user") {
        setGroundMapOrientationMode("user");
        centerOnUserNow();
        return;
    }

    setGroundMapFollowUser(false);
    enterManualOrientationMode();
    updateCenterControlAppearance();
}

function trackedAssetTitle() {
    return window.__gs26_tracked_asset_title || trackedAssetLabel || "Tracked Asset";
}

function makeMapStyle(tilesUrl, effectiveMaxNativeZoom) {
    const rasterTemplate = shouldUseNativeTileTemplate(tilesUrl)
        ? String(tilesUrl || "")
        : tileProtocolTemplate();
    const sourceMaxZoom = Math.max(
        MIN_ZOOM,
        Math.floor(Number(currentMaxZoom) || Number(effectiveMaxNativeZoom) || DEFAULT_MAX_NATIVE_ZOOM)
    );
    return {
        version: 8,
        sources: {
            [TILE_SOURCE_ID]: {
                type: "raster",
                tiles: [rasterTemplate],
                tileSize: 256,
                bounds: [NA_BOUNDS.lonMin, NA_BOUNDS.latMin, NA_BOUNDS.lonMax, NA_BOUNDS.latMax],
                minzoom: MIN_ZOOM,
                maxzoom: sourceMaxZoom,
            },
            [GUIDE_SOURCE_ID]: {
                type: "geojson",
                data: emptyFeatureCollection(),
            },
            [ROCKET_SOURCE_ID]: {
                type: "geojson",
                data: emptyFeatureCollection(),
            },
            [USER_SOURCE_ID]: {
                type: "geojson",
                data: emptyFeatureCollection(),
            },
            [USER_HEADING_SOURCE_ID]: {
                type: "geojson",
                data: emptyFeatureCollection(),
            },
        },
        layers: [
            {
                id: TILE_LAYER_ID,
                type: "raster",
                source: TILE_SOURCE_ID,
                paint: {
                    "raster-opacity": 1,
                },
            },
            {
                id: GUIDE_LAYER_ID,
                type: "line",
                source: GUIDE_SOURCE_ID,
                paint: {
                    "line-color": "#ef4444",
                    "line-width": 3,
                    "line-opacity": 0.95,
                },
            },
            {
                id: ROCKET_LAYER_ID,
                type: "symbol",
                source: ROCKET_SOURCE_ID,
                layout: {
                    "icon-image": ROCKET_ICON_IMAGE_ID,
                    "icon-size": 0.8,
                    "icon-allow-overlap": true,
                    "icon-ignore-placement": true,
                },
            },
            {
                id: USER_LAYER_ID,
                type: "symbol",
                source: USER_SOURCE_ID,
                layout: {
                    "icon-image": USER_ICON_IMAGE_ID,
                    "icon-size": 1.3,
                    "icon-allow-overlap": true,
                    "icon-ignore-placement": true,
                },
            },
            {
                id: USER_HEADING_LAYER_ID,
                type: "symbol",
                source: USER_HEADING_SOURCE_ID,
                filter: ["==", ["geometry-type"], "Point"],
                layout: {
                    "icon-image": USER_HEADING_IMAGE_ID,
                    "icon-size": 0.9,
                    "icon-offset": [0, -30],
                    "icon-allow-overlap": true,
                    "icon-ignore-placement": true,
                    "icon-rotation-alignment": "map",
                    "icon-rotate": ["get", "bearing"],
                },
            },
        ],
    };
}

function addOverlayControls() {
    if (!groundMap || mapNavigationControl) return;
    const maplibre = getMapLibre();
    mapNavigationControl = new maplibre.NavigationControl({
        showZoom: true,
        showCompass: false,
        visualizePitch: false,
    });
    groundMap.addControl(mapNavigationControl, "top-right");

    class CenterOnUserControl {
        onAdd() {
            const container = document.createElement("div");
            container.className = "maplibregl-ctrl maplibregl-ctrl-group";

            const button = document.createElement("button");
            button.type = "button";
            button.className = "gs26-map-center-control";
            button.title = "Center On Me";
            button.setAttribute("aria-label", "Center On Me");
            button.innerHTML = '<span class="gs26-map-center-control-icon" aria-hidden="true"></span>';
            button.addEventListener("click", (event) => {
                event.preventDefault();
                activateLocateControl();
            });

            container.appendChild(button);
            this._container = container;
            this._button = button;
            updateCenterControlAppearance();
            return container;
        }

        onRemove() {
            if (this._container && this._container.parentNode) {
                this._container.parentNode.removeChild(this._container);
            }
            this._container = null;
            this._button = null;
        }
    }

    class NorthIndicatorControl {
        onAdd() {
            const container = document.createElement("div");
            container.className = "maplibregl-ctrl maplibregl-ctrl-group";

            const button = document.createElement("button");
            button.type = "button";
            button.className = "gs26-map-north-control";
            button.hidden = true;
            button.title = "Reset North Up";
            button.setAttribute("aria-label", "Reset North Up");
            button.innerHTML = '<span class="gs26-map-north-control-icon" aria-hidden="true"></span>';
            button.addEventListener("click", (event) => {
                event.preventDefault();
                setGroundMapOrientationMode("north");
                updateNorthControlAppearance();
            });

            container.appendChild(button);
            this._container = container;
            this._button = button;
            this._icon = button.querySelector(".gs26-map-north-control-icon");
            updateNorthControlAppearance();
            return container;
        }

        onRemove() {
            if (this._container && this._container.parentNode) {
                this._container.parentNode.removeChild(this._container);
            }
            this._container = null;
            this._button = null;
            this._icon = null;
        }
    }

    mapCenterControl = new CenterOnUserControl();
    groundMap.addControl(mapCenterControl, "top-right");
    mapNorthControl = new NorthIndicatorControl();
    groundMap.addControl(mapNorthControl, "top-right");
}

function installCustomGestureHooks() {
    if (!groundMap) return;
    const canvas = groundMap.getCanvasContainer ? groundMap.getCanvasContainer() : groundMap.getCanvas();
    if (!canvas || canvas.__gs26_custom_gestures_installed) return;

    const controller = new AbortController();
    const signal = controller.signal;
    const state = {
        shiftRotateActive: false,
        shiftRotateStartX: 0,
        shiftRotateStartBearing: 0,
        dragPanWasEnabled: false,
        touchGesture: null,
    };

    const stopShiftRotate = () => {
        if (!state.shiftRotateActive) return;
        state.shiftRotateActive = false;
        if (state.dragPanWasEnabled && groundMap && groundMap.dragPan && !groundMap.dragPan.isEnabled()) {
            groundMap.dragPan.enable();
        }
        state.dragPanWasEnabled = false;
    };

    canvas.__gs26_custom_gestures_installed = true;
    canvas.__gs26_custom_gestures_controller = controller;

    canvas.addEventListener("wheel", (event) => {
        if (!event.shiftKey || !groundMap) return;
        event.preventDefault();
        unlockMapInteraction({force: true, dropFollow: false, dropOrientation: true});
        enterManualOrientationMode();
        mapBearingDeg = normalizeAngle(mapBearingDeg + event.deltaY * 0.18);
        applyMapOrientation();
    }, {passive: false, signal});

    canvas.addEventListener("mousedown", (event) => {
        if (!groundMap || event.button !== 0) return;
        if (!event.shiftKey) {
            unlockMapInteraction({force: true, dropFollow: true, dropOrientation: true});
            return;
        }
        event.preventDefault();
        unlockMapInteraction({force: true, dropFollow: false, dropOrientation: true});
        enterManualOrientationMode();
        state.shiftRotateActive = true;
        state.shiftRotateStartX = event.clientX;
        state.shiftRotateStartBearing = mapBearingDeg;
        state.dragPanWasEnabled = !!(groundMap.dragPan && groundMap.dragPan.isEnabled());
        if (state.dragPanWasEnabled) {
            groundMap.dragPan.disable();
        }
    }, {signal});

    window.addEventListener("mousemove", (event) => {
        if (!state.shiftRotateActive || !groundMap) return;
        const dx = event.clientX - state.shiftRotateStartX;
        mapBearingDeg = normalizeAngle(state.shiftRotateStartBearing + dx * 0.45);
        applyMapOrientation();
    }, {signal});

    window.addEventListener("mouseup", () => {
        stopShiftRotate();
    }, {signal});

    canvas.addEventListener("touchstart", (event) => {
        if (!groundMap) return;
        if (event.touches.length === 1) {
            unlockMapInteraction({force: true, dropFollow: true, dropOrientation: true});
            state.touchGesture = null;
            return;
        }
        if (event.touches.length !== 2) {
            state.touchGesture = null;
            return;
        }
        event.preventDefault();
        state.touchGesture = {
            startAngle: touchAngle(event.touches),
            startDistance: Math.max(1, touchDistance(event.touches)),
            startMidpoint: touchMidpoint(event.touches),
            startCenter: groundMap.getCenter(),
            startZoom: groundMap.getZoom(),
            startBearing: mapBearingDeg,
        };
    }, {passive: false, signal});

    canvas.addEventListener("touchmove", (event) => {
        if (!groundMap || !state.touchGesture || event.touches.length !== 2) return;
        event.preventDefault();

        const currentMidpoint = touchMidpoint(event.touches);
        const midpointDx = currentMidpoint.x - state.touchGesture.startMidpoint.x;
        const midpointDy = currentMidpoint.y - state.touchGesture.startMidpoint.y;
        const startCenterPoint = groundMap.project(state.touchGesture.startCenter);
        const nextCenter = groundMap.unproject([
            startCenterPoint.x - midpointDx,
            startCenterPoint.y - midpointDy,
        ]);

        const currentDistance = Math.max(1, touchDistance(event.touches));
        const distanceScale = Math.max(0.25, Math.min(4.0, currentDistance / state.touchGesture.startDistance));
        const nextZoom = Math.min(
            currentMaxZoom,
            Math.max(MIN_ZOOM, state.touchGesture.startZoom + Math.log2(distanceScale))
        );

        const currentAngle = touchAngle(event.touches);
        const angleDelta = shortestAngleDiff(state.touchGesture.startAngle, currentAngle);
        let nextBearing = state.touchGesture.startBearing;
        if (Math.abs(angleDelta) >= TWO_TOUCH_ROTATE_THRESHOLD_DEG) {
            unlockMapInteraction({force: true, dropFollow: false, dropOrientation: true});
            enterManualOrientationMode();
            nextBearing = normalizeAngle(state.touchGesture.startBearing - angleDelta);
        }

        mapBearingDeg = nextBearing;
        markInternalCameraUpdate(16);
        groundMap.jumpTo({
            center: [nextCenter.lng, nextCenter.lat],
            zoom: nextZoom,
            bearing: nextBearing,
        });
        updateUserMarkerRotation();
        rememberMapView();
        updateCenterControlAppearance();
    }, {passive: false, signal});

    const clearTouchGesture = () => {
        state.touchGesture = null;
    };
    canvas.addEventListener("touchend", clearTouchGesture, {signal});
    canvas.addEventListener("touchcancel", clearTouchGesture, {signal});

}

function installMapHooks() {
    if (!groundMap) return;

    groundMap.on("load", () => {
        ensureMapMarkerImages();
        mapReady = true;
        syncRocketGuideLine(lastRocketLatLng, userMarkerDisplayedLatLng || lastUserLatLng);
        syncPointSource(ROCKET_SOURCE_ID, lastRocketLatLng);
        syncPointSource(USER_SOURCE_ID, userMarkerDisplayedLatLng || lastUserLatLng);
        syncUserHeadingIndicator();
        scheduleHighResTilePrefetch();
        scheduleTileZoomDiscovery();
    });

    groundMap.on("moveend", () => {
        rememberMapView();
        scheduleHighResTilePrefetch();
        scheduleTileZoomDiscovery();
    });
    groundMap.on("zoomend", () => {
        suppressFollowCameraUntilMs = 0;
        suppressHighResPrefetch(1200);
        rememberMapView();
        scheduleHighResTilePrefetch();
        if (followUserEnabled) {
            applyFollowUserIfPossible();
        }
    });
    groundMap.on("rotateend", () => {
        mapBearingDeg = normalizeAngle(groundMap.getBearing());
        if (!isInternalCameraUpdate() && orientationMode !== "manual") {
            orientationMode = "manual";
            try {
                window.__gs26_map_orientation_mode = orientationMode;
            } catch (e) {
            }
        }
        updateUserMarkerRotation();
        rememberMapView();
        updateCenterControlAppearance();
    });

    groundMap.on("drag", () => {
        updateUserMarkerRotation();
    });

    groundMap.on("dragstart", () => {
        suppressHighResPrefetch(2500);
        unlockMapInteraction({force: true, dropFollow: true, dropOrientation: true});
    });
    groundMap.on("zoomstart", () => {
        suppressHighResPrefetch(2500);
        suppressFollowCameraUntilMs = Date.now() + 1000;
        unlockMapInteraction({force: true, dropFollow: false, dropOrientation: false});
    });
    groundMap.on("rotatestart", () => {
        suppressHighResPrefetch(2500);
        if (isInternalCameraUpdate()) return;
        unlockMapInteraction({force: true, dropFollow: false, dropOrientation: true});
    });
    for (const eventName of ["pitchstart"]) {
        groundMap.on(eventName, disableFollowUserFromMapInteraction);
    }

    try {
        const canvas = groundMap.getCanvas();
        if (canvas && !canvas.__gs26_follow_disable_hooks) {
            canvas.__gs26_follow_disable_hooks = true;
            canvas.addEventListener("wheel", () => {
                unlockMapInteraction({force: true, dropFollow: false, dropOrientation: false});
            }, {passive: true});
        }
    } catch (e) {
    }
}

function resetMapObjects() {
    cancelUserMarkerAnimation();
    if (groundMap) {
        try {
            const canvas = groundMap.getCanvasContainer ? groundMap.getCanvasContainer() : null;
            if (canvas && canvas.__gs26_custom_gestures_controller) {
                canvas.__gs26_custom_gestures_controller.abort();
                delete canvas.__gs26_custom_gestures_controller;
                delete canvas.__gs26_custom_gestures_installed;
            }
        } catch (e) {
        }
    }
    if (tileZoomDiscoveryTimer) {
        clearTimeout(tileZoomDiscoveryTimer);
        tileZoomDiscoveryTimer = null;
    }
    mapReady = false;
    mapNavigationControl = null;
    mapCenterControl = null;
    mapNorthControl = null;
    userMarkerDisplayedLatLng = null;
}

function initGroundMap(tilesUrl, centerLat, centerLon, zoom, maxNativeZoom, assetTitle) {
    ensureMarkerStylesOnce();
    requestPersistentTileStorage();
    initCompassOnce();
    ensureMapProtocolOnce();
    mapInitStartedAtMs = Date.now();

    const previousTilesUrl = currentTilesUrl;
    const previousMaxNativeZoom = currentMaxNativeZoom;
    const nextConfiguredMaxNativeZoom = clampMaxNativeZoom(maxNativeZoom);
    const nextConfiguredMaxDisplayZoom = clampMaxDisplayZoom(
        Number(window.__gs26_max_display_zoom),
        nextConfiguredMaxNativeZoom
    );
    const nextMaxNativeZoom = effectiveMaxNativeZoomFor(nextConfiguredMaxNativeZoom);
    const nextMaxZoom = nextConfiguredMaxDisplayZoom;
    const needsFullRecreate =
        !!groundMap && (
            previousTilesUrl !== tilesUrl ||
            previousMaxNativeZoom !== nextMaxNativeZoom
        );

    trackedAssetLabel = assetTitle || trackedAssetTitle();
    currentTilesUrl = tilesUrl;
    configuredMaxNativeZoom = nextConfiguredMaxNativeZoom;
    configuredMaxDisplayZoom = nextConfiguredMaxDisplayZoom;
    currentMaxNativeZoom = nextMaxNativeZoom;
    currentMaxZoom = nextMaxZoom;
    currentPrefetchKey = null;
    scheduleTileCacheSweep(tilesUrl);

    const container = document.getElementById("ground-map");
    if (!container) return;

    const desiredZoom = lastMapView ? lastMapView.zoom : zoom;
    const clampedZoom = Math.min(currentMaxZoom, Math.max(MIN_ZOOM, desiredZoom));
    const startCenter = lastMapView
        ? [lastMapView.lon, lastMapView.lat]
        : [centerLon, centerLat];
    const startBearing = orientationMode === "north" ? 0 : mapBearingDeg;

    if (!needsFullRecreate && groundMap && groundMap.getContainer && groundMap.getContainer() === container) {
        groundMap.resize();
        groundMap.setMaxZoom(currentMaxZoom);
        applyMapOrientation();
        applyPendingCenterIfPossible();
        applyFollowUserIfPossible();
        scheduleTileZoomDiscovery();
        return;
    }

    if (groundMap) {
        try {
            groundMap.remove();
        } catch (e) {
        }
        groundMap = null;
        window.__gs26_ground_map = null;
    }
    resetMapObjects();

    const maplibre = getMapLibre();
    groundMap = new maplibre.Map({
        container,
        style: makeMapStyle(currentTilesUrl, currentMaxNativeZoom),
        center: startCenter,
        zoom: clampedZoom,
        bearing: startBearing,
        minZoom: MIN_ZOOM,
        maxZoom: currentMaxZoom,
        dragRotate: true,
        dragPan: true,
        scrollZoom: true,
        doubleClickZoom: true,
        boxZoom: false,
        touchZoomRotate: true,
        touchPitch: false,
        pitchWithRotate: false,
        maxPitch: 0,
        attributionControl: false,
        cooperativeGestures: false,
        renderWorldCopies: false,
    });
    groundMap.invalidateSize = () => {
        try {
            groundMap.resize();
        } catch (e) {
        }
    };
    if (groundMap.dragPan && !groundMap.dragPan.isEnabled()) {
        groundMap.dragPan.enable();
    }
    if (groundMap.scrollZoom && !groundMap.scrollZoom.isEnabled()) {
        groundMap.scrollZoom.enable();
    }
    if (groundMap.touchZoomRotate && typeof groundMap.touchZoomRotate.enable === "function") {
        groundMap.touchZoomRotate.enable();
    }
    if (groundMap.touchPitch && typeof groundMap.touchPitch.disable === "function") {
        groundMap.touchPitch.disable();
    }
    addOverlayControls();
    installMapHooks();
    installCustomGestureHooks();
    rememberMapView();
    window.__gs26_ground_map = groundMap;
    syncRequestedMapControlState();

    if (lastUserLatLng) {
        userMarkerDisplayedLatLng = [lastUserLatLng[0], lastUserLatLng[1]];
    }

    syncPointSource(ROCKET_SOURCE_ID, lastRocketLatLng);
    syncPointSource(USER_SOURCE_ID, userMarkerDisplayedLatLng || lastUserLatLng);
    syncUserHeadingIndicator();

    applyPendingCenterIfPossible();
    applyFollowUserIfPossible();
    applyMapOrientation();
    updateCenterControlAppearance();
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

    if (hasRocket) {
        syncPointSource(ROCKET_SOURCE_ID, lastRocketLatLng);
    }

    if (hasUser) {
        let userMarkerCreated = false;
        if (!Array.isArray(userMarkerDisplayedLatLng)) {
            userMarkerDisplayedLatLng = [lastUserLatLng[0], lastUserLatLng[1]];
            setUserMarkerVisualLatLng(userMarkerDisplayedLatLng);
            userMarkerCreated = true;
        } else {
            animateUserMarkerTo(lastUserLatLng);
        }
        syncRequestedMapControlState();
        if (userMarkerCreated && followUserEnabled) {
            applyFollowUserIfPossible();
        }
    }

    syncRocketGuideLine(
        hasRocket ? lastRocketLatLng : null,
        hasUser ? (userMarkerDisplayedLatLng || lastUserLatLng) : null
    );
    applyPendingCenterIfPossible();
    applyMapOrientation();
    if (hasRocket || hasUser) {
        scheduleTileZoomDiscovery();
        scheduleHighResTilePrefetch();
    }
}

function centerGroundMapOn(lat, lon) {
    if (!groundMap) return;
    markInternalCameraUpdate(250);
    groundMap.jumpTo({
        center: [lon, lat],
        bearing: mapBearingDeg,
    });
    rememberMapView();
    scheduleTileZoomDiscovery();
    scheduleHighResTilePrefetch();
}

function getLastUserLatLng() {
    if (!lastUserLatLng) return null;
    return {lat: lastUserLatLng[0], lon: lastUserLatLng[1]};
}

(function pinGroundStation26() {
    const api = (window.GS26 = window.GS26 || {});

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
    api.syncRequestedMapControlState = syncRequestedMapControlState;
    api.initCompassOnce = initCompassOnce;
    api.handleOrientation = handleOrientation;
    api.getMapLibre = getMapLibre;
    api.normalizeAngle = normalizeAngle;
    api.shortestAngleDiff = shortestAngleDiff;
    api.ensureMarkerStylesOnce = ensureMarkerStylesOnce;
    api.rememberMapView = rememberMapView;
    api.updateUserMarkerRotation = updateUserMarkerRotation;
    api.setGroundMapUserHeading = setGroundMapUserHeading;
    api.applyMapOrientation = applyMapOrientation;
    api.syncRocketGuideLine = syncRocketGuideLine;

    api.state = api.state || {};
    Object.assign(api.state, {
        NA_BOUNDS,
        MIN_ZOOM,
        DEFAULT_MAX_NATIVE_ZOOM,
        get groundMap() {
            return groundMap;
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
    window.syncRequestedMapControlState = api.syncRequestedMapControlState;
    window.scheduleHighResTilePrefetch = api.scheduleHighResTilePrefetch;

    window.__gs26_ground_station_loaded = true;
    try {
        window.dispatchEvent(new CustomEvent("gs26-ground-map-ready"));
    } catch (e) {
    }
    window.__gs26_ground_map_cache_state = {...tilePrefetchState};
    window.__gs26_ground_map_cache_ready = false;
})();
