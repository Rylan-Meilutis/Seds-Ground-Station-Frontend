//
// GroundStation26 map runtime
// Map engine: MapLibre GL JS
//

let groundMap = null;
let userMarkerDisplayedLatLng = null;
let userMarkerAnimationFrame = null;
let userMarkerAnimation = null;
let userMarkerHasLiveFix = false;
let headingAnimationFrame = null;
let headingAnimationLastFrameAt = 0;
let browserHeadingSyncTimer = null;
let followCameraFrame = null;
let pendingFollowCameraLatLng = null;
let followCameraLastFrameAt = 0;
let followCameraVisualPoint = null;
let followCameraVelocityXPps = 0;
let followCameraVelocityYPps = 0;
let followCameraCenterLocked = false;
let currentTilesUrl = null;
let configuredMaxNativeZoom = null;
let configuredMaxDisplayZoom = null;
let currentMaxNativeZoom = null;
let currentMaxZoom = null;
let currentMinZoom = null;
let pendingRuntimeMinZoom = null;
let runtimeMinZoomTimer = null;
let lastRocketLatLng = null;
let lastUserLatLng = null;
let prefetchRocketLatLng = null;
let prefetchUserLatLng = null;
let rocketGpsStability = null;
let userGpsStability = null;
let lastMapView = null;
let lastMapZoom = null;
let pendingRestoreZoom = null;
let tileCacheSweepTimer = null;
let tilePrefetchTimer = null;
let tileZoomDiscoveryTimer = null;
let tileZoomDiscoveryKey = "";
let tileZoomDiscoveryRunId = 0;
let currentPrefetchKey = null;
let tilePrefetchRunId = 0;
let autoHighResPrefetchCompleted = false;
let tileTrackingPrefetchTimer = null;
let tileTrackingPrefetchInterval = null;
let currentTrackingPrefetchKey = null;
let tileTrackingPrefetchRunId = 0;
let tileTrackingPrefetchActive = false;
let tileCacheUsageMeasureTimer = null;
let tileMemoryCacheBytes = 0;
let tilePersistentClearPromise = null;
let browserTileCacheQueue = Promise.resolve();
const tileMissingCache = new Map();
const tileFetchInflight = new Map();
let transparentTileArrayBufferPromise = null;
let wheelRotateFrame = null;
let wheelRotateTargetBearing = null;
let wheelGestureMode = null;
let wheelGestureLastAtMs = 0;
let zoomButtonTargetZoom = null;
let zoomButtonAnimationFrame = null;
let zoomButtonAnimationLastAtMs = 0;
let followZoomAnimationFrame = null;
let followZoomHoldUntilMs = 0;
let prefetchSuppressedUntilMs = 0;
let mapInitStartedAtMs = 0;
let mapInitTimingLogged = false;
let firstTileTimingLogged = false;
let mapFirstPaintGateActive = false;
let mapFirstPaintTimer = null;
let mapFirstPaintTargetZoom = null;
let mapMainThreadWatchdogTimer = null;
let mapMainThreadWatchdogLastTickAt = 0;
let mapInitPhase = "idle";
let mapInitTrace = [];
let lastPersistedMapStateAtMs = 0;
let lastMarkerSyncAtMs = 0;
let markerSyncTimer = null;
let pendingMarkerSync = null;
let followUserEnabled = false;
let orientationMode = "north";
let mapBearingDeg = 0;
let suppressFollowCameraUntilMs = 0;
let suppressFollowDisableUntilMs = 0;
let followEnableGuardUntilMs = 0;
let internalCameraUpdateUntilMs = 0;
let orientationModeAnimationUntilMs = 0;
let orientationModeSettleUntilMs = 0;
let suppressManualOrientationDropUntilMs = 0;
let pendingUserUpRealign = false;
let userHeadingDegRaw = null;
let userHeadingDeg = null;
let userHeadingDisplayDeg = null;
let userHeadingCameraDeg = null;
let userHeadingIndicatorDeg = null;
let userHeadingArrowDeg = null;
let nativeHeadingDeg = null;
let deviceHeadingDeg = null;
let lastHeadingVisualSyncAtMs = 0;
let compassInitialized = false;
const BROWSER_HEADING_SYNC_INTERVAL_MS = 100;
let maplibreProtocolInstalled = false;
let mapRuntimeGuardsInstalled = false;
let mapReady = false;
let trackedAssetLabel = "Tracked Asset";
let mapNavigationControl = null;
let mapCenterControl = null;
let mapNorthControl = null;
const TILE_PROTOCOL = "gs26map";
const SOURCE_EMPTY_KEY = "__empty__";

let tilePrefetchState = {
    key: "", state: "idle", detail: "", pending: 0, completed: 0, failed: 0, lastStartedAt: 0, lastCompletedAt: 0,
};

let tilePrefetchEstimateState = {
    tiles: 0,
    estimatedBytes: 0,
    estimatedTileBytes: 96 * 1024,
    budgetBytes: 500 * 1024 * 1024,
    tooLarge: false,
    updatedAt: 0,
};
let tilePrefetchContextState = {
    userAvailable: false,
    rocketAvailable: false,
    userStatus: "waiting",
    rocketStatus: "waiting",
    summaryStatus: "waiting",
    userMessage: "Waiting for user location.",
    rocketMessage: "Waiting for rocket telemetry.",
    summaryMessage: "Waiting for user location and rocket telemetry.",
    updatedAt: 0,
};
let tilePrefetchEstimatedTileBytes = 96 * 1024;
let tilePrefetchSizeSampleToken = "";

const MIN_ZOOM = 0;
const DEFAULT_SAFE_MIN_ZOOM = 5;
const DEFAULT_MAX_NATIVE_ZOOM = 12;
const MAX_NATIVE_ZOOM_LIMIT = 18;
const DEFAULT_MAX_OVERZOOM_DELTA = 1;
const MAX_DISPLAY_ZOOM_LIMIT = MAX_NATIVE_ZOOM_LIMIT + DEFAULT_MAX_OVERZOOM_DELTA;
const HIGH_RES_PREFETCH_DEFAULT_RADIUS_M = 1609.344;
const HIGH_RES_PREFETCH_MIN_RADIUS_M = 100;
const HIGH_RES_PREFETCH_MAX_RADIUS_M = 20000;
const HIGH_RES_PREFETCH_CONCURRENCY = 8;
const HIGH_RES_PREFETCH_STATE_UPDATE_INTERVAL_MS = 250;
const HIGH_RES_PREFETCH_VIEWPORT_BUFFER_TILES = 5;
const HIGH_RES_PREFETCH_LOCATION_BUFFER_TILES = 5;
const HIGH_RES_PREFETCH_FOCUS_ZOOM_DELTA = 3;
const HIGH_RES_PREFETCH_STARTUP_DELAY_MS = 500;
const HIGH_RES_PREFETCH_IDLE_DELAY_MS = 2500;
const HIGH_RES_PREFETCH_IDLE_DELAY_MS_WEB = 12000;
const HIGH_RES_PREFETCH_IDLE_DELAY_MS_NATIVE_DESKTOP = 20000;
const MAP_VISIBLE_PREFETCH_SUSPEND_DETAIL = "Auto-prefetch is paused while the live map is visible.";
const TRACKING_PREFETCH_TILE_RADIUS = 14;
const TRACKING_PREFETCH_ZOOM_DELTA = 3;
const TRACKING_PREFETCH_ZOOM_OUT_VIEWPORT_LEVELS = 3;
const TRACKING_PREFETCH_ZOOM_IN_VIEWPORT_LEVELS = 3;
const TRACKING_PREFETCH_VIEWPORT_BUFFER_TILES = 4;
const TRACKING_PREFETCH_MAX_TILES = 3200;
const TRACKING_PREFETCH_INTERVAL_MS = 2500;
const TRACKING_PREFETCH_INTERVAL_MS_WEB = 12000;
const TRACKING_PREFETCH_DELAY_MS = 80;
const TRACKING_PREFETCH_DELAY_MS_WEB = 2000;
const TRACKING_PREFETCH_CONCURRENCY = 3;
const TRACKING_PREFETCH_CONCURRENCY_WEB = 1;
const HIGH_RES_PREFETCH_CONCURRENCY_WEB = 1;
const HIGH_RES_PREFETCH_CONCURRENCY_NATIVE_DESKTOP = 1;
const NATIVE_DESKTOP_PREFETCH_YIELD_MS = 32;

function configuredPrefetchRadiusM(kind) {
    const key = kind === "rocket" ? "__gs26_prefetch_rocket_radius_m" : "__gs26_prefetch_user_radius_m";
    const value = Number(typeof window !== "undefined" ? window[key] : NaN);
    if (!Number.isFinite(value)) return HIGH_RES_PREFETCH_DEFAULT_RADIUS_M;
    return Math.max(HIGH_RES_PREFETCH_MIN_RADIUS_M, Math.min(HIGH_RES_PREFETCH_MAX_RADIUS_M, value));
}

const MARKER_SYNC_MIN_INTERVAL_MS = 100;
const ZOOM_BUTTON_ANIMATION_MS = 240;
const ZOOM_BUTTON_UNITS_PER_SECOND = 6.5;
const ZOOM_BUTTON_MAX_DISTANCE_SPEEDUP = 2.6;
const ZOOM_BUTTON_SETTLE_EPSILON = 0.002;
const ORIENTATION_MODE_ANIMATION_MS = 520;
const WHEEL_ROTATE_DEG_PER_PIXEL = 0.18;
const WHEEL_ROTATE_EASE = 0.24;
const WHEEL_ROTATE_SETTLE_DEG = 0.08;
const WHEEL_ROTATE_AXIS_DOMINANCE = 1.0;
const WHEEL_GESTURE_LOCK_MS = 180;
const WHEEL_ZOOM_LIMIT_EPSILON = 0.001;
const CACHE_SWEEP_DELAY_MS = 15000;
const TILE_CACHE_ENABLED_BY_DEFAULT = true;
const TILE_CACHE_PREFIXES = ["gs26-tiles-v1:", "gs26-tiles-v2:"];
const TILE_CACHE_PREFIX = "gs26-tiles-v2:";
const TILE_CACHE_USAGE_BYTES_STORAGE_KEY = "gs26_tile_cache_usage_bytes";
const TILE_MEMORY_CACHE_MAX_BYTES = 8 * 1024 * 1024;
const TILE_CACHE_MAX_TILE_BYTES = 2 * 1024 * 1024;
const TILE_CACHE_DEFAULT_BUDGET_BYTES = 500 * 1024 * 1024;
const TILE_PREFETCH_ESTIMATED_TILE_BYTES = 96 * 1024;
const TILE_MISSING_CACHE_TTL_MS = 5 * 60 * 1000;
const TRANSPARENT_TILE_BYTES = Uint8Array.from([137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4, 0, 0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 252, 255, 31, 0, 3, 3, 2, 0, 239, 191, 167, 219, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,]);
const MAP_FIRST_PAINT_GATE_TIMEOUT_MS = 180;
const MAP_FIRST_PAINT_FADE_MS = 45;
const MAP_MAIN_THREAD_WATCHDOG_INTERVAL_MS = 250;
const MAP_MAIN_THREAD_STALL_WARN_MS = 1500;
const MAP_INIT_TRACE_MAX_ENTRIES = 120;
const STARTUP_CACHE_WARM_MAX_TILES = 16;
const STARTUP_CACHE_WARM_CONCURRENCY = 4;
const STARTUP_ZOOM_DISCOVERY_DELAY_MS = 1200;
const STARTUP_TILE_CACHE_LOOKUP_BUDGET_MS = 45;
const STARTUP_CACHE_BYPASS_MS = 10000;
const tileCacheHandles = new Map();
const tileMemoryCache = new Map();
const USER_MARKER_SMOOTH_MIN_MS = 180;
const USER_MARKER_SMOOTH_MAX_MS = 900;
const USER_MARKER_SMOOTH_SKIP_M = 0.04;
const USER_MARKER_PREDICTION_MAX_MS = 900;
const USER_MARKER_PREDICTION_RATIO = 0.55;
const USER_MARKER_PREDICTION_STALE_RATIO = 1.4;
const USER_MARKER_RATE_MIN_CATCHUP_MS = 220;
const USER_MARKER_RATE_MAX_CATCHUP_MS = 900;
const USER_MARKER_VISUAL_MIN_SPEED_MPS = 0.2;
const USER_MARKER_VISUAL_MAX_SPEED_MPS = 90;
const USER_MARKER_VISUAL_SPEED_GAIN = 1.12;
const USER_MARKER_VISUAL_CATCHUP_MS = 520;
const USER_MARKER_FRAME_STALL_SNAP_MS = 220;
const USER_GPS_SNAP_DISTANCE_M = 120;
const USER_GPS_SNAP_SPEED_MPS = 45;
const USER_ORIENTATION_DEADZONE_DEG = 2.2;
const USER_ORIENTATION_INPUT_DEADZONE_DEG = 0.9;
const USER_ORIENTATION_CAMERA_SETTLE_DEG = 0.035;
const USER_ORIENTATION_CAMERA_CATCHUP_MS = 105;
const USER_ORIENTATION_MAX_STEP_DEG = 6.0;
const USER_ORIENTATION_INDICATOR_DEADZONE_DEG = 0.8;
const USER_ORIENTATION_EASE_MS = 320;
const USER_ORIENTATION_DISPLAY_DEADZONE_DEG = 0.18;
const USER_ORIENTATION_DISPLAY_CATCHUP_MS = 160;
const USER_ORIENTATION_INDICATOR_CATCHUP_MS = 180;
const USER_ORIENTATION_ARROW_CATCHUP_MS = 95;
const USER_ORIENTATION_ARROW_DEADZONE_DEG = 0.12;
const USER_ORIENTATION_SMALL_ERROR_GAIN = 0.025;
const USER_HEADING_VISUAL_SYNC_MIN_MS = 33;
const GPS_STABLE_FIX_REQUIRED = 1;
const GPS_STABLE_DISTANCE_M = 50;
const FOLLOW_CAMERA_SETTLE_EPSILON_PX = 0.2;
const FOLLOW_CAMERA_MIN_STEP_PX = 90.0;
const FOLLOW_CAMERA_MAX_STEP_PX = 900.0;
const FOLLOW_CAMERA_STEP_DISTANCE_RATIO = 0.42;
const FOLLOW_CAMERA_CATCHUP_MS = 110;
const FOLLOW_CAMERA_LOCK_DISTANCE_PX = 6.0;
const FOLLOW_CAMERA_FRAME_STALL_SNAP_MS = 180;
const TILE_SOURCE_ID = "gs26-raster-source";
const TILE_LAYER_ID = "gs26-raster-layer";
const MAP_BACKGROUND_LAYER_ID = "gs26-map-background-layer";
const MAP_BACKGROUND_COLOR = "#111827";
const GUIDE_SOURCE_ID = "gs26-guide-source";
const GUIDE_LAYER_ID = "gs26-guide-layer";
const USER_SOURCE_ID = "gs26-user-source";
const USER_LAYER_ID = "gs26-user-layer";
const ROCKET_SOURCE_ID = "gs26-rocket-source";
const ROCKET_LAYER_ID = "gs26-rocket-layer";
const USER_HEADING_SOURCE_ID = "gs26-user-heading-source";
const USER_HEADING_LAYER_ID = "gs26-user-heading-layer";
const MAP_STATE_STORAGE_KEY = "gs26_ground_map_state_v3";
const MAP_MAX_ZOOM_STORAGE_KEY = "gs26_ground_map_max_zoom_v1";
const USER_ICON_IMAGE_ID = "gs26-user-icon";
const ROCKET_ICON_IMAGE_ID = "gs26-rocket-icon";
const USER_HEADING_IMAGE_ID = "gs26-user-heading-icon";

const NA_BOUNDS = {
    lonMin: -170.0, latMin: 5.0, lonMax: -50.0, latMax: 83.0,
};

const TWO_TOUCH_ROTATE_THRESHOLD_DEG = 12;

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

function isIosPlatform() {
    if (typeof navigator === "undefined") return false;
    const userAgent = navigator.userAgent || "";
    const platform = navigator.platform || "";
    return /iPad|iPhone|iPod/i.test(userAgent) || /iPad|iPhone|iPod/i.test(platform) || (platform === "MacIntel" && navigator.maxTouchPoints > 1);
}

function isDesktopNativeMapRuntime() {
    return !isBrowserHostedMapRuntime() && !isIosPlatform() && !isAndroidPlatform();
}

function normalizeAngle(deg) {
    let value = Number(deg) || 0;
    value %= 360;
    if (value < 0) value += 360;
    return value;
}

function reportMapRuntimeError(label, error) {
    try {
        if (isExpectedTileFallbackError(error)) return;
        console.warn("[GS26 map]", label, error);
    } catch (e) {
    }
}

function safeMapCallback(label, fn) {
    return function safeMapCallbackWrapper(...args) {
        try {
            const result = fn.apply(this, args);
            if (result && typeof result.then === "function") {
                result.catch((error) => {
                    reportMapRuntimeError(label, error);
                });
            }
            return result;
        } catch (error) {
            reportMapRuntimeError(label, error);
            return undefined;
        }
    };
}

function installMapRuntimeGuardsOnce() {
    if (mapRuntimeGuardsInstalled || typeof window === "undefined") return;
    mapRuntimeGuardsInstalled = true;
    window.addEventListener("unhandledrejection", (event) => {
        const reason = event && event.reason;
        reportMapRuntimeError("unhandled promise rejection", reason);
    });
}

function isExpectedTileFallbackError(error) {
    const message = String(error && (error.message || error) || "");
    return message.includes("tile fetch failed:") || message.includes("tile known missing") || message.includes("tile cache miss") || message.includes("unsupported tile url") || message.includes("tile url missing");
}

function mapDebugLoggingEnabled() {
    try {
        return window.__gs26_map_debug === true || window.localStorage?.getItem("gs26_map_debug") === "on";
    } catch (e) {
        return false;
    }
}

function pushMapTrace(label, extra = {}) {
    const entry = {
        atMs: Date.now(),
        sinceInitMs: mapInitStartedAtMs > 0 ? Math.round(Date.now() - mapInitStartedAtMs) : null,
        label, ...extra,
    };
    mapInitPhase = label;
    mapInitTrace.push(entry);
    if (mapInitTrace.length > MAP_INIT_TRACE_MAX_ENTRIES) {
        mapInitTrace.splice(0, mapInitTrace.length - MAP_INIT_TRACE_MAX_ENTRIES);
    }
}

function logMapRuntimeBoundary(_label, _extra = {}) {
}

function startMapMainThreadWatchdog(source) {
    if (mapMainThreadWatchdogTimer != null) return;
    mapMainThreadWatchdogLastTickAt = Date.now();
    pushMapTrace("watchdog-start", {source});
    mapMainThreadWatchdogTimer = setInterval(() => {
        const now = Date.now();
        const deltaMs = now - mapMainThreadWatchdogLastTickAt;
        mapMainThreadWatchdogLastTickAt = now;
        if (deltaMs < MAP_MAIN_THREAD_STALL_WARN_MS) return;
        const snapshot = mapInitTrace.slice(-12);
        try {
            window.__gs26_map_stall = {
                detectedAtMs: now, blockedForMs: deltaMs, phase: mapInitPhase, trace: snapshot,
            };
        } catch (e) {
        }
    }, MAP_MAIN_THREAD_WATCHDOG_INTERVAL_MS);
}

function stopMapMainThreadWatchdog(reason) {
    if (mapMainThreadWatchdogTimer == null) return;
    clearInterval(mapMainThreadWatchdogTimer);
    mapMainThreadWatchdogTimer = null;
    pushMapTrace("watchdog-stop", {reason});
}

function markInternalCameraUpdate(durationMs) {
    internalCameraUpdateUntilMs = Date.now() + Math.max(0, Number(durationMs) || 0);
}

function isInternalCameraUpdate() {
    return Date.now() < internalCameraUpdateUntilMs;
}

function orientationModeAnimationRemainingMs() {
    return Math.max(0, orientationModeAnimationUntilMs - Date.now());
}

function isOrientationModeAnimationActive() {
    return orientationModeAnimationRemainingMs() > 0;
}

function isOrientationModeSettling() {
    return Date.now() < orientationModeSettleUntilMs;
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
    return typeof window !== "undefined" && typeof window.caches !== "undefined" && typeof window.fetch === "function";
}

function tilePrefetchSupported() {
    return typeof window !== "undefined" && typeof window.fetch === "function";
}

function shouldSerializeBrowserTileCacheOps() {
    return isBrowserHostedMapRuntime();
}

async function yieldBrowserTileWork() {
    if (isBrowserHostedMapRuntime()) {
        await new Promise((resolve) => setTimeout(resolve, 0));
        return;
    }
    if (isDesktopNativeMapRuntime()) {
        await new Promise((resolve) => setTimeout(resolve, NATIVE_DESKTOP_PREFETCH_YIELD_MS));
    }
}

function runBrowserTileCacheOp(task) {
    if (!shouldSerializeBrowserTileCacheOps()) {
        return Promise.resolve().then(task);
    }
    const wrapped = async () => {
        await yieldBrowserTileWork();
        try {
            return await task();
        } finally {
            await yieldBrowserTileWork();
        }
    };
    const next = browserTileCacheQueue.then(wrapped, wrapped);
    browserTileCacheQueue = next.catch(() => {
    });
    return next;
}

function tileCacheEnabled() {
    try {
        if (typeof window !== "undefined" && window.__gs26_tile_cache_disabled === true) {
            return false;
        }
        if (typeof window !== "undefined" && typeof window.__gs26_tile_cache_enabled === "boolean") {
            return window.__gs26_tile_cache_enabled;
        }
        if (typeof window !== "undefined" && window.localStorage) {
            const stored = window.localStorage.getItem("gs26_tile_cache_enabled");
            if (stored === "off") return false;
            if (stored === "on") return true;
        }
    } catch (e) {
    }
    return TILE_CACHE_ENABLED_BY_DEFAULT;
}

function persistentTileCacheEnabled() {
    if (!tileCacheEnabled()) return false;
    try {
        if (!isBrowserHostedMapRuntime()) return true;
        if (window.__gs26_enable_web_persistent_tile_cache === true) return true;
    } catch (e) {
    }
    return !isBrowserHostedMapRuntime();
}

function configuredCacheBudgetBytes() {
    try {
        const direct = Number(typeof window !== "undefined" ? window.__gs26_cache_budget_bytes : NaN);
        if (Number.isFinite(direct) && direct > 0) {
            return Math.max(1, direct);
        }
        if (typeof window !== "undefined" && window.localStorage) {
            const mb = Number(window.localStorage.getItem("gs_cache_budget_mb"));
            if (Number.isFinite(mb) && mb > 0) {
                return Math.max(1, mb * 1024 * 1024);
            }
        }
    } catch (e) {
    }
    return TILE_CACHE_DEFAULT_BUDGET_BYTES;
}

function rememberTileSizeSample(bytes) {
    const value = Number(bytes);
    if (!Number.isFinite(value) || value <= 0 || value > TILE_CACHE_MAX_TILE_BYTES) return;
    tilePrefetchEstimatedTileBytes = Math.max(1024, Math.round((tilePrefetchEstimatedTileBytes * 0.75) + (value * 0.25)));
}

function setTilePrefetchEstimate(plan, sampleTileSize = true) {
    const tiles = plan && Number.isFinite(Number(plan.totalTiles))
        ? Math.max(0, Number(plan.totalTiles))
        : (plan && Array.isArray(plan.coords) ? plan.coords.length : 0);
    const breakdown = plan && plan.breakdown ? plan.breakdown : {};
    const userTiles = Math.max(0, Number(breakdown.userTiles) || 0);
    const rocketTiles = Math.max(0, Number(breakdown.rocketTiles) || 0);
    const combinedTiles = Math.max(0, Number(breakdown.combinedTiles) || tiles);
    const budgetBytes = configuredCacheBudgetBytes();
    const estimatedTileBytes = Math.max(1024, tilePrefetchEstimatedTileBytes || TILE_PREFETCH_ESTIMATED_TILE_BYTES);
    const estimatedBytes = tiles * estimatedTileBytes;
    const context = publishTilePrefetchContextState();
    tilePrefetchEstimateState = {
        tiles,
        estimatedBytes,
        estimatedTileBytes,
        userTiles,
        userEstimatedBytes: userTiles * estimatedTileBytes,
        rocketTiles,
        rocketEstimatedBytes: rocketTiles * estimatedTileBytes,
        combinedTiles,
        combinedEstimatedBytes: combinedTiles * estimatedTileBytes,
        budgetBytes,
        tooLarge: estimatedBytes > budgetBytes,
        userAvailable: context.userAvailable,
        rocketAvailable: context.rocketAvailable,
        userStatus: context.userStatus,
        rocketStatus: context.rocketStatus,
        summaryStatus: context.summaryStatus,
        userMessage: context.userMessage,
        rocketMessage: context.rocketMessage,
        summaryMessage: context.summaryMessage,
        updatedAt: Date.now(),
    };
    try {
        window.__gs26_ground_map_prefetch_estimate = {...tilePrefetchEstimateState};
    } catch (e) {
    }
    if (sampleTileSize) {
        schedulePrefetchTileSizeSample(plan);
    }
    return tilePrefetchEstimateState;
}

function refreshTilePrefetchEstimate(options = {}) {
    const forceSample = options && options.sampleTileSize === true;
    const enabled = mapPrefetchEnabled();
    const context = publishTilePrefetchContextState();
    const hasRunnableContext = context.userAvailable || context.rocketAvailable;
    const canRunNow = enabled && hasRunnableContext && (shouldRunAutomaticHighResPrefetch() ? shouldRunHighResBrowserMapPrefetch() : shouldRunBrowserMapPrefetch());
    let plan = emptyTilePrefetchPlan();
    if (canRunNow) {
        plan = shouldRunAutomaticHighResPrefetch() ? buildHighResPrefetchPlan() : buildTrackingPrefetchPlan();
    }
    const estimate = setTilePrefetchEstimate(plan, forceSample);
    if (!enabled) {
        estimate.summaryStatus = "disabled";
        estimate.summaryMessage = "Map prefetch is disabled.";
        estimate.userMessage = context.userAvailable ? "Ready" : "Waiting for user location.";
        estimate.rocketMessage = context.rocketAvailable ? "Ready" : "Waiting for rocket telemetry.";
        estimate.tooLarge = false;
        try {
            window.__gs26_ground_map_prefetch_estimate = {...estimate};
        } catch (e) {
        }
    } else if (canRunNow && !shouldRunAutomaticHighResPrefetch()) {
        estimate.summaryStatus = "tracking";
        estimate.summaryMessage = context.userAvailable && !context.rocketAvailable ? "Tracking prefetch only. Rocket tiles are deferred until telemetry appears." : "Tracking prefetch only.";
        try {
            window.__gs26_ground_map_prefetch_estimate = {...estimate};
        } catch (e) {
        }
    } else if (canRunNow && context.userAvailable && !context.rocketAvailable && estimate.combinedTiles > 0) {
        estimate.summaryMessage = "Current estimate excludes rocket tiles. Cache need may increase when rocket telemetry appears.";
        try {
            window.__gs26_ground_map_prefetch_estimate = {...estimate};
        } catch (e) {
        }
    }
    return estimate;
}

function storedTileCacheUsageBytes() {
    try {
        const direct = Number(typeof window !== "undefined" ? window.__gs26_tile_cache_usage_bytes : NaN);
        if (Number.isFinite(direct) && direct >= 0) {
            return Math.max(0, Math.round(direct));
        }
        if (typeof window !== "undefined" && window.localStorage) {
            const stored = Number(window.localStorage.getItem(TILE_CACHE_USAGE_BYTES_STORAGE_KEY));
            if (Number.isFinite(stored) && stored >= 0) {
                return Math.max(0, Math.round(stored));
            }
        }
    } catch (e) {
    }
    return 0;
}

function setStoredTileCacheUsageBytes(bytes) {
    const next = Math.max(0, Math.round(Number(bytes) || 0));
    try {
        window.__gs26_tile_cache_usage_bytes = next;
        if (window.localStorage) {
            window.localStorage.setItem(TILE_CACHE_USAGE_BYTES_STORAGE_KEY, String(next));
        }
    } catch (e) {
    }
}

function bumpStoredTileCacheUsageBytes(deltaBytes) {
    const delta = Math.round(Number(deltaBytes) || 0);
    if (!Number.isFinite(delta) || delta === 0) return;
    setStoredTileCacheUsageBytes(storedTileCacheUsageBytes() + delta);
}

async function measurePersistentTileCacheUsageBytes() {
    return storedTileCacheUsageBytes();
}

function schedulePersistentTileCacheUsageMeasure(delayMs = 0) {
    void delayMs;
}

function schedulePrefetchTileSizeSample(plan) {
    if (!plan || !Array.isArray(plan.coords) || !plan.coords.length) return;
    const tilesUrl = effectivePrefetchTilesUrl();
    if (!tilesUrl || !tileFetchAllowedForUrl(tilesUrl)) return;
    const coord = plan.coords.find((item) => item && Number.isFinite(item.z) && Number.isFinite(item.x) && Number.isFinite(item.y));
    if (!coord) return;
    const url = resolvePrefetchTileUrl(tilesUrl, coord.z, coord.x, coord.y);
    if (!url || isKnownMissingTile(tileCacheName(tilesUrl), url)) return;
    const token = `${plan.key || ""}|${url}`;
    if (tilePrefetchSizeSampleToken === token) return;
    tilePrefetchSizeSampleToken = token;

    Promise.resolve().then(async () => {
        let sampleBytes = NaN;
        try {
            const cached = await readCachedTileArrayBuffer(tileCacheName(tilesUrl), url);
            if (cached) {
                sampleBytes = cached.byteLength;
            }
        } catch (e) {
        }
        try {
            if (!Number.isFinite(sampleBytes) || sampleBytes <= 0) {
                const head = await fetch(url, {method: "HEAD"});
                if (head && head.ok && head.headers) {
                    sampleBytes = Number(head.headers.get("content-length"));
                }
            }
        } catch (e) {
        }
        if (!Number.isFinite(sampleBytes) || sampleBytes <= 0) {
            try {
                const response = await fetch(url);
                if (response && response.ok) {
                    const clone = response.clone();
                    const data = await response.arrayBuffer();
                    sampleBytes = data.byteLength;
                    if (tileCacheEnabled() && tileCacheSupported() && sampleBytes <= TILE_CACHE_MAX_TILE_BYTES) {
                        try {
                            const cache = await openTileCache(tileCacheName(tilesUrl));
                            if (cache) {
                                await cache.put(tileCacheRequestKey(url), clone);
                                bumpStoredTileCacheUsageBytes(sampleBytes);
                            }
                        } catch (e) {
                            schedulePersistentTileCacheUsageMeasure(0);
                        }
                    }
                }
            } catch (e) {
            }
        }
        if (tilePrefetchSizeSampleToken !== token) return;
        rememberTileSizeSample(sampleBytes);
        setTilePrefetchEstimate(plan, false);
    });
}

function tileFetchAllowedForUrl(url) {
    return /^https?:/i.test(String(url || "")) || /^gs26:\/\//i.test(String(url || ""));
}

function tileCacheRequestKey(url) {
    const raw = String(url || "");
    if (/^https?:/i.test(raw)) return raw;
    return `https://gs26.tile-cache.local/${encodeURIComponent(raw)}`;
}

function canonicalTileSourceKey(tilesUrl) {
    return String(tilesUrl || "").trim();
}

function hashTileSourceKey(tilesUrl) {
    const value = canonicalTileSourceKey(tilesUrl);
    let hash = 0x811c9dc5;
    for (let i = 0; i < value.length; i++) {
        hash ^= value.charCodeAt(i);
        hash = Math.imul(hash, 0x01000193) >>> 0;
    }
    return hash.toString(16).padStart(8, "0");
}

function isGroundMapTileCacheName(name) {
    return TILE_CACHE_PREFIXES.some((prefix) => String(name || "").startsWith(prefix));
}

function cloneTileArrayBuffer(data) {
    if (!data || typeof data.byteLength !== "number") return data;
    try {
        return data.slice(0);
    } catch (e) {
        return data;
    }
}

function tileMemoryKey(cacheName, url) {
    return `${cacheName}\n${tileCacheRequestKey(url)}`;
}

function transparentTileFallbackArrayBuffer() {
    return TRANSPARENT_TILE_BYTES.slice().buffer;
}

async function transparentTileArrayBuffer() {
    if (!transparentTileArrayBufferPromise) {
        transparentTileArrayBufferPromise = Promise.resolve(transparentTileFallbackArrayBuffer());
    }
    return cloneTileArrayBuffer(await transparentTileArrayBufferPromise);
}

function tileMissingKey(cacheName, url) {
    return tileMemoryKey(cacheName, url);
}

function rememberMissingTile(cacheName, url, status = 0) {
    if (!cacheName || !url) return;
    tileMissingCache.set(tileMissingKey(cacheName, url), {
        expiresAt: Date.now() + TILE_MISSING_CACHE_TTL_MS, status,
    });
}

function forgetMissingTile(cacheName, url) {
    if (!cacheName || !url) return;
    tileMissingCache.delete(tileMissingKey(cacheName, url));
}

function isKnownMissingTile(cacheName, url) {
    if (!cacheName || !url) return false;
    const key = tileMissingKey(cacheName, url);
    const entry = tileMissingCache.get(key);
    if (!entry) return false;
    if (Date.now() >= entry.expiresAt) {
        tileMissingCache.delete(key);
        return false;
    }
    return true;
}

function readTileMemoryCache(cacheName, url) {
    if (!tileCacheEnabled()) return null;
    const key = tileMemoryKey(cacheName, url);
    const entry = tileMemoryCache.get(key);
    if (!entry || !entry.data) return null;
    tileMemoryCache.delete(key);
    tileMemoryCache.set(key, entry);
    return cloneTileArrayBuffer(entry.data);
}

function hasTileMemoryCache(cacheName, url) {
    if (!tileCacheEnabled()) return false;
    const key = tileMemoryKey(cacheName, url);
    const entry = tileMemoryCache.get(key);
    if (!entry || !entry.data) return false;
    tileMemoryCache.delete(key);
    tileMemoryCache.set(key, entry);
    return true;
}

function writeTileMemoryCache(cacheName, url, data) {
    if (!tileCacheEnabled()) return;
    if (!data || typeof data.byteLength !== "number") return;
    if (data.byteLength > TILE_CACHE_MAX_TILE_BYTES) return;
    if (TILE_MEMORY_CACHE_MAX_BYTES <= 0) return;
    const key = tileMemoryKey(cacheName, url);
    const existing = tileMemoryCache.get(key);
    if (existing && Number.isFinite(existing.bytes)) {
        tileMemoryCacheBytes = Math.max(0, tileMemoryCacheBytes - existing.bytes);
        tileMemoryCache.delete(key);
    }
    const bytes = data.byteLength;
    tileMemoryCache.set(key, {data: cloneTileArrayBuffer(data), bytes});
    tileMemoryCacheBytes += bytes;
    while (tileMemoryCacheBytes > TILE_MEMORY_CACHE_MAX_BYTES && tileMemoryCache.size > 0) {
        const firstKey = tileMemoryCache.keys().next().value;
        const removed = tileMemoryCache.get(firstKey);
        tileMemoryCache.delete(firstKey);
        if (removed && Number.isFinite(removed.bytes)) {
            tileMemoryCacheBytes = Math.max(0, tileMemoryCacheBytes - removed.bytes);
        }
    }
}

async function openTileCache(cacheName) {
    if (!tileCacheSupported() || !tileCacheEnabled()) return null;
    if (tilePersistentClearPromise) {
        try {
            await tilePersistentClearPromise;
        } catch (e) {
        }
    }
    if (!tileCacheHandles.has(cacheName)) {
        tileCacheHandles.set(cacheName, caches.open(cacheName).catch((e) => {
            tileCacheHandles.delete(cacheName);
            throw e;
        }));
    }
    return await tileCacheHandles.get(cacheName);
}

function inflightTileFetchKey(cacheName, url) {
    return `${cacheName}\n${tileCacheRequestKey(url)}`;
}

function runInflightTileFetch(cacheName, url, factory) {
    const key = inflightTileFetchKey(cacheName, url);
    const existing = tileFetchInflight.get(key);
    if (existing) return existing;
    const created = Promise.resolve()
        .then(factory)
        .finally(() => {
            if (tileFetchInflight.get(key) === created) {
                tileFetchInflight.delete(key);
            }
        });
    tileFetchInflight.set(key, created);
    return created;
}

function requestPersistentTileStorage() {
    try {
        if (!tileCacheEnabled()) return;
        if (!navigator.storage || typeof navigator.storage.persist !== "function") return;
        if (window.__gs26_tile_storage_persist_requested) return;
        window.__gs26_tile_storage_persist_requested = true;
        navigator.storage.persist().catch(() => {
        });
    } catch (e) {
    }
}

function clearTileRuntimeCaches(options = {}) {
    tileMemoryCache.clear();
    tileMemoryCacheBytes = 0;
    tileMissingCache.clear();
    tileCacheHandles.clear();
    tileFetchInflight.clear();
    if (tileCacheSweepTimer) {
        clearTimeout(tileCacheSweepTimer);
        tileCacheSweepTimer = null;
    }
    suppressHighResPrefetch(500);
    stopTrackingTilePrefetch();
    currentPrefetchKey = null;
    autoHighResPrefetchCompleted = false;
    setTilePrefetchState({
        key: "", state: "idle", pending: 0, completed: 0, failed: 0, lastStartedAt: 0, lastCompletedAt: 0,
    });
    if (options.clearZoomMetadata === true) {
        try {
            window.__gs26_ground_map_max_zoom_json = "";
            if (window.localStorage) {
                window.localStorage.removeItem(MAP_MAX_ZOOM_STORAGE_KEY);
            }
        } catch (e) {
        }
    }
}

async function clearPersistentTileCaches(options = {}) {
    if (!tileCacheSupported() || typeof caches.keys !== "function") return;
    const keepName = options.keepName || "";
    const keys = await caches.keys();
    await Promise.all(keys
        .filter((key) => isGroundMapTileCacheName(key) && key !== keepName)
        .map((key) => caches.delete(key)));
    if (!keepName) {
        setStoredTileCacheUsageBytes(0);
    } else {
        schedulePersistentTileCacheUsageMeasure(0);
    }
}

async function clearAllGroundMapTileCaches() {
    clearTileRuntimeCaches({clearZoomMetadata: true});
    tilePersistentClearPromise = clearPersistentTileCaches()
        .finally(() => {
            tilePersistentClearPromise = null;
        });
    await tilePersistentClearPromise;
}

function invalidateTileCachesForUrlChange() {
    clearTileRuntimeCaches({clearZoomMetadata: true});
    tilePersistentClearPromise = Promise.resolve().then(async () => {
        try {
            await clearPersistentTileCaches();
        } catch (e) {
            if (mapDebugLoggingEnabled()) console.warn("[GS26 map] url cache invalidation failed", e);
        }
    }).finally(() => {
        tilePersistentClearPromise = null;
    });
}

function shouldRunAutomaticHighResPrefetch() {
    return !autoHighResPrefetchCompleted;
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
    prefetchSuppressedUntilMs = Math.max(prefetchSuppressedUntilMs, Date.now() + Math.max(0, Number(ms) || 0));
    if (tilePrefetchTimer) {
        cancelIdleDelay(tilePrefetchTimer);
        tilePrefetchTimer = null;
    }
    tilePrefetchRunId += 1;
}

function stopTrackingTilePrefetch() {
    if (tileTrackingPrefetchTimer) {
        cancelIdleDelay(tileTrackingPrefetchTimer);
        tileTrackingPrefetchTimer = null;
    }
    if (tileTrackingPrefetchInterval) {
        clearInterval(tileTrackingPrefetchInterval);
        tileTrackingPrefetchInterval = null;
    }
    currentTrackingPrefetchKey = null;
    tileTrackingPrefetchRunId += 1;
    tileTrackingPrefetchActive = false;
}

function tileCacheName(tilesUrl) {
    const label = canonicalTileSourceKey(tilesUrl)
        .replace(/[^a-z0-9]+/gi, "_")
        .replace(/^_+|_+$/g, "")
        .toLowerCase()
        .slice(0, 48) || "default";
    return `${TILE_CACHE_PREFIX}${hashTileSourceKey(tilesUrl)}:${label}`;
}

function resolveTileUrl(z, x, y) {
    if (!currentTilesUrl) return "";
    const coord = normalizeTileCoord(z, x, y);
    if (!coord) return "";
    return String(currentTilesUrl)
        .replace("{z}", String(coord.z))
        .replace("{x}", String(coord.x))
        .replace("{y}", String(coord.y));
}

function effectivePrefetchTilesUrl() {
    return String(currentTilesUrl || window.__gs26_tiles_url || "").trim();
}

function effectivePrefetchMaxNativeZoom(tilesUrl) {
    const candidates = [currentMaxNativeZoom, window.__gs26_max_native_zoom, loadPersistedMaxNativeZoom(tilesUrl), DEFAULT_MAX_NATIVE_ZOOM,];
    let best = null;
    for (const raw of candidates) {
        if (raw == null || raw === "") continue;
        const value = Number(raw);
        if (Number.isFinite(value)) {
            const zoom = clampMaxNativeZoom(value);
            best = best == null ? zoom : Math.max(best, zoom);
        }
    }
    return best == null ? DEFAULT_MAX_NATIVE_ZOOM : best;
}

function resolvePrefetchTileUrl(tilesUrl, z, x, y) {
    const coord = normalizeTileCoord(z, x, y);
    if (!coord) return "";
    return String(tilesUrl || "")
        .replace("{z}", String(coord.z))
        .replace("{x}", String(coord.x))
        .replace("{y}", String(coord.y));
}

function tileProtocolTemplate() {
    return `${TILE_PROTOCOL}://tiles/{z}/{x}/{y}.jpg`;
}

function isBrowserHostedMapRuntime() {
    try {
        const protocol = String(window.location && window.location.protocol || "");
        return /^https?:$/i.test(protocol);
    } catch (e) {
        return false;
    }
}

function shouldUseNativeTileTemplate(tilesUrl) {
    const url = String(tilesUrl || "");
    if (isBrowserHostedMapRuntime() && /^https?:\/\//i.test(url)) {
        return true;
    }
    return /^gs26:\/\//i.test(url) && !isIosPlatform() && !tileCacheEnabled();
}

function tilesUseNativeProxy() {
    return shouldUseNativeTileTemplate(currentTilesUrl);
}

function effectiveMinZoom() {
    const runtimeMin = Number.isFinite(currentMinZoom) ? Math.max(DEFAULT_SAFE_MIN_ZOOM, currentMinZoom) : DEFAULT_SAFE_MIN_ZOOM;
    return Math.max(MIN_ZOOM, runtimeMin);
}

function raiseRuntimeMinZoom(minZoom) {
    const nextMin = Math.max(DEFAULT_SAFE_MIN_ZOOM, Math.floor(Number(minZoom)));
    if (!Number.isFinite(nextMin)) return;
    if (Number.isFinite(currentMinZoom) && currentMinZoom >= nextMin) return;
    currentMinZoom = nextMin;
    try {
        if (groundMap && typeof groundMap.setMinZoom === "function") {
            groundMap.setMinZoom(effectiveMinZoom());
        }
        if (groundMap && typeof groundMap.getZoom === "function" && groundMap.getZoom() < effectiveMinZoom()) {
            markInternalCameraUpdate(160);
            groundMap.jumpTo({
                zoom: effectiveMinZoom(), bearing: mapBearingDeg,
            });
            rememberMapView();
        }
    } catch (e) {
    }
    persistTileZoomRange(effectivePrefetchTilesUrl(), currentMinZoom, Number.isFinite(currentMaxNativeZoom) ? currentMaxNativeZoom : DEFAULT_MAX_NATIVE_ZOOM);
    updateZoomControlAppearance();
}

function handleMissingTileCoord(coord) {
    if (!coord || !Number.isFinite(coord.z)) return;
    const zoom = groundMap && typeof groundMap.getZoom === "function" ? Number(groundMap.getZoom()) : NaN;
    if (!Number.isFinite(zoom) || zoom > coord.z + 0.25) return;
    const nextMin = Math.max(DEFAULT_SAFE_MIN_ZOOM, Math.floor(coord.z + 1));
    if (Number.isFinite(currentMinZoom) && currentMinZoom >= nextMin) return;
    pendingRuntimeMinZoom = Number.isFinite(pendingRuntimeMinZoom) ? Math.max(pendingRuntimeMinZoom, nextMin) : nextMin;
    if (runtimeMinZoomTimer != null) return;
    runtimeMinZoomTimer = setTimeout(safeMapCallback("runtime min zoom timer", () => {
        runtimeMinZoomTimer = null;
        const target = pendingRuntimeMinZoom;
        pendingRuntimeMinZoom = null;
        raiseRuntimeMinZoom(target);
    }), 0);
}

function normalizeTileCoord(z, x, y) {
    let zi = Math.floor(Number(z));
    if (!Number.isFinite(zi)) return null;
    const minZoom = effectiveMinZoom();
    if (zi < minZoom) {
        return null;
    }
    const scale = Math.pow(2, zi);
    if (!Number.isFinite(scale) || scale < 1) return null;
    let xi = Math.floor(Number(x));
    let yi = Math.floor(Number(y));
    if (!Number.isFinite(xi) || !Number.isFinite(yi)) return null;
    xi = Math.max(0, Math.min(scale - 1, xi));
    yi = Math.max(0, Math.min(scale - 1, yi));
    return {z: zi, x: xi, y: yi};
}

function mapPrefetchEnabled() {
    if (!tileCacheEnabled()) return false;
    try {
        if (typeof window !== "undefined" && typeof window.__gs26_prefetch_enabled === "boolean") {
            return window.__gs26_prefetch_enabled;
        }
        if (typeof window !== "undefined" && window.localStorage) {
            return (window.localStorage.getItem("gs_map_prefetch_enabled") || "on") !== "off";
        }
    } catch (e) {
    }
    return true;
}

function parseTileProtocolRequest(url) {
    const match = String(url || "").match(/^gs26map:\/\/tiles\/(-?\d+)\/(-?\d+)\/(-?\d+)\.jpg(?:\?.*)?$/i);
    if (!match) return null;
    return normalizeTileCoord(match[1], match[2], match[3]);
}

function mapFirstPaintNativeZoomFor(zoom) {
    const requested = Math.floor(Number(zoom));
    const nativeMax = Number.isFinite(currentMaxNativeZoom) ? currentMaxNativeZoom : DEFAULT_MAX_NATIVE_ZOOM;
    if (!Number.isFinite(requested)) return effectiveMinZoom();
    return Math.max(effectiveMinZoom(), Math.min(nativeMax, requested));
}

function setMapContainerFirstPaintHidden(hidden) {
    const container = document.getElementById("ground-map");
    if (!container) return;
    try {
        container.style.backgroundColor = MAP_BACKGROUND_COLOR;
    } catch (e) {
    }
    try {
        container.style.transition = hidden ? "none" : `opacity ${MAP_FIRST_PAINT_FADE_MS}ms ease-out`;
        container.style.opacity = "1";
    } catch (e) {
    }
}

function finishMapFirstPaintGate(reason) {
    if (!mapFirstPaintGateActive) return;
    mapFirstPaintGateActive = false;
    if (mapFirstPaintTimer != null) {
        clearTimeout(mapFirstPaintTimer);
        mapFirstPaintTimer = null;
    }
    mapFirstPaintTargetZoom = null;
    setMapContainerFirstPaintHidden(false);
    logMapInitTiming("first-paint", {reason});
}

function maybeFinishMapFirstPaintFromTile(coords) {
    if (!mapFirstPaintGateActive || !coords) return;
    const targetZoom = Number.isFinite(mapFirstPaintTargetZoom) ? mapFirstPaintTargetZoom : MIN_ZOOM;
    if (Number(coords.z) >= targetZoom) {
        requestAnimationFrame(safeMapCallback("map first paint frame", () => {
            requestAnimationFrame(safeMapCallback("map first paint frame", () => finishMapFirstPaintGate("target-tile")));
        }));
    }
}

function startMapFirstPaintGate(zoom) {
    if (mapFirstPaintTimer != null) {
        clearTimeout(mapFirstPaintTimer);
        mapFirstPaintTimer = null;
    }
    mapFirstPaintGateActive = true;
    mapFirstPaintTargetZoom = mapFirstPaintNativeZoomFor(zoom);
    setMapContainerFirstPaintHidden(true);
    mapFirstPaintTimer = setTimeout(safeMapCallback("map first paint timeout", () => {
        finishMapFirstPaintGate("timeout");
    }), MAP_FIRST_PAINT_GATE_TIMEOUT_MS);
}

function warmInitialMapTilesFromCache(tilesUrl, center, zoom, container = null) {
    try {
        if (typeof window !== "undefined" && window.location && /^https?:$/i.test(String(window.location.protocol || ""))) {
            return Promise.resolve(false);
        }
    } catch (e) {
    }
    if (!tileCacheSupported() || !tileFetchAllowedForUrl(tilesUrl) || !Array.isArray(center)) return Promise.resolve(false);
    const lon = Number(center[0]);
    const lat = Number(center[1]);
    if (!Number.isFinite(lat) || !Number.isFinite(lon)) return Promise.resolve(false);
    const nativeZoom = mapFirstPaintNativeZoomFor(zoom);
    let coords = [];
    if (container) {
        coords = tileCoordsForViewportAround(lat, lon, nativeZoom, 1);
    }
    if (!coords.length) {
        coords = tileCoordsAroundTileRadius(lat, lon, nativeZoom, 1);
    }
    coords = coords.slice(0, STARTUP_CACHE_WARM_MAX_TILES);
    if (!coords.length) return Promise.resolve(false);
    const cacheName = tileCacheName(tilesUrl);
    return Promise.resolve().then(async () => {
        let foundCachedTile = false;
        let nextIndex = 0;
        const workerCount = Math.max(1, Math.min(STARTUP_CACHE_WARM_CONCURRENCY, coords.length));
        const workers = Array.from({length: workerCount}, async () => {
            while (nextIndex < coords.length) {
                const coord = coords[nextIndex++];
                if (currentTilesUrl !== tilesUrl) return;
                const url = resolvePrefetchTileUrl(tilesUrl, coord.z, coord.x, coord.y);
                if (!url) continue;
                const cached = await readCachedTileArrayBuffer(cacheName, url);
                if (cached) {
                    foundCachedTile = true;
                    maybeFinishMapFirstPaintFromTile(coord);
                }
            }
        });
        await Promise.all(workers);
        if (foundCachedTile) {
            logMapInitTiming("first-tile", {source: "startup-cache-warm"});
            if (groundMap && typeof groundMap.triggerRepaint === "function") {
                groundMap.triggerRepaint();
            }
        }
        return foundCachedTile;
    }).catch(() => false);
}

function usePersistedCachedZoomForStartup(tilesUrl, desiredZoom) {
    const cachedNativeZoom = loadPersistedMaxNativeZoom(tilesUrl);
    const requestedNativeZoom = Math.floor(Number(desiredZoom));
    if (!Number.isFinite(cachedNativeZoom) || !Number.isFinite(requestedNativeZoom)) return;
    if (cachedNativeZoom < requestedNativeZoom || currentMaxNativeZoom >= requestedNativeZoom) return;
    currentMaxNativeZoom = Math.max(currentMaxNativeZoom, Math.min(cachedNativeZoom, requestedNativeZoom));
    currentMaxZoom = Math.max(Number.isFinite(currentMaxZoom) ? currentMaxZoom : DEFAULT_SAFE_MIN_ZOOM, Math.min(MAX_DISPLAY_ZOOM_LIMIT, currentMaxNativeZoom + DEFAULT_MAX_OVERZOOM_DELTA));
    try {
        window.__gs26_max_native_zoom = currentMaxNativeZoom;
    } catch (e) {
    }
}

function renderAfterStartupCacheWarm(warmPromise) {
    Promise.resolve(warmPromise).then((foundCachedTile) => {
        if (!foundCachedTile) return;
        if (groundMap && typeof groundMap.triggerRepaint === "function") {
            groundMap.triggerRepaint();
        }
    });
}

function ensureMapProtocolOnce() {
    if (maplibreProtocolInstalled) return;
    const maplibre = getMapLibre();
    if (typeof maplibre.addProtocol !== "function") return;

    maplibre.addProtocol(TILE_PROTOCOL, async (params) => {
        let coords = null;
        let url = "";
        let cacheName = "";
        try {
            coords = parseTileProtocolRequest(params && params.url);
            if (!coords) {
                return {data: await transparentTileArrayBuffer()};
            }
            url = resolveTileUrl(coords.z, coords.x, coords.y);
            if (!url) {
                return {data: await transparentTileArrayBuffer()};
            }
            cacheName = tileCacheName(currentTilesUrl);
            if (isKnownMissingTile(cacheName, url)) {
                handleMissingTileCoord(coords);
                return {data: await transparentTileArrayBuffer()};
            }
            const cacheLookupOptions = mapFirstPaintGateActive ? {timeoutMs: STARTUP_TILE_CACHE_LOOKUP_BUDGET_MS} : undefined;
            const cached = await readCachedTileArrayBuffer(cacheName, url, cacheLookupOptions);
            if (cached) {
                logMapInitTiming("first-tile", {source: "cache", url});
                maybeFinishMapFirstPaintFromTile(coords);
                return {data: cached};
            }
            const data = await fetchAndCacheTileArrayBuffer(cacheName, url, {skipCacheLookup: true});
            logMapInitTiming("first-tile", {source: "fetch", url});
            maybeFinishMapFirstPaintFromTile(coords);
            return {data};
        } catch (primaryError) {
            try {
                const cached = await readCachedTileArrayBuffer(cacheName, url);
                if (cached) {
                    logMapInitTiming("first-tile", {source: "cache-after-error", url});
                    maybeFinishMapFirstPaintFromTile(coords);
                    return {data: cached};
                }
                rememberMissingTile(cacheName, url, 0);
                handleMissingTileCoord(coords);
                maybeFinishMapFirstPaintFromTile(coords);
            } catch (fallbackError) {
                reportMapRuntimeError("tile protocol fallback failed", fallbackError);
            }
            if (!isExpectedTileFallbackError(primaryError)) {
                reportMapRuntimeError("tile protocol returned transparent tile", primaryError);
            }
            return {data: await transparentTileArrayBuffer()};
        }
    });
    maplibreProtocolInstalled = true;
}

function logMapInitTiming(label, extra = {}) {
    if (!mapDebugLoggingEnabled()) return;
    if (label === "first-tile") {
        if (firstTileTimingLogged) return;
        firstTileTimingLogged = true;
    }
    if (mapInitTimingLogged && label !== "first-tile" && label !== "first-paint") return;
    try {
        const elapsed = mapInitStartedAtMs > 0 ? Math.round(Date.now() - mapInitStartedAtMs) : null;
        console.info("[GS26 map timing]", label, {elapsedMs: elapsed, ...extra});
    } catch (e) {
    }
    if (label === "load") {
        mapInitTimingLogged = true;
    }
}

function shouldBypassPersistentTileCacheRead() {
    if (mapInitStartedAtMs <= 0) return false;
    return (Date.now() - mapInitStartedAtMs) < STARTUP_CACHE_BYPASS_MS;
}

async function readCachedTileArrayBuffer(cacheName, url, options = {}) {
    if (!tileCacheEnabled() || !tileCacheSupported() || !url) return null;
    const hot = readTileMemoryCache(cacheName, url);
    if (hot) return hot;
    if (options.allowPersistent !== true && shouldBypassPersistentTileCacheRead()) {
        return null;
    }
    const originalPersistentPromise = readPersistentTileArrayBuffer(cacheName, url);
    let persistentPromise = originalPersistentPromise;
    const timeoutMs = Number(options.timeoutMs);
    if (Number.isFinite(timeoutMs) && timeoutMs > 0) {
        originalPersistentPromise.then((delayedPersistent) => {
            if (delayedPersistent) writeTileMemoryCache(cacheName, url, delayedPersistent);
        }).catch(() => {
        });
        persistentPromise = Promise.race([originalPersistentPromise, new Promise((resolve) => setTimeout(() => resolve(null), timeoutMs)),]);
    }
    const persistent = await persistentPromise;
    if (!persistent) return null;
    writeTileMemoryCache(cacheName, url, persistent);
    return cloneTileArrayBuffer(persistent);
}

async function readPersistentTileArrayBuffer(cacheName, url) {
    if (!tileCacheEnabled() || !tileCacheSupported() || !url) return null;
    try {
        const cached = await runBrowserTileCacheOp(async () => {
            const cache = await openTileCache(cacheName);
            return await cache.match(tileCacheRequestKey(url), {ignoreVary: true});
        });
        if (!cached || !cached.ok) return null;
        return await cached.arrayBuffer();
    } catch (e) {
        if (mapDebugLoggingEnabled()) console.warn("[GS26 map] cache read failed", url, e);
        return null;
    }
}

async function fetchAndCacheTileArrayBuffer(cacheName, url, options = {}) {
    if (!url) throw new Error("tile url missing");
    if (!tileFetchAllowedForUrl(url)) throw new Error(`unsupported tile url: ${url}`);
    if (isKnownMissingTile(cacheName, url)) {
        throw new Error("tile known missing");
    }

    if (!tileCacheEnabled() || !tileCacheSupported()) {
        await yieldBrowserTileWork();
        const response = await fetch(url);
        if (!response.ok) {
            if (response.status === 404 || response.status === 410) {
                rememberMissingTile(cacheName, url, response.status);
            }
            throw new Error(`tile fetch failed: ${response.status}`);
        }
        await yieldBrowserTileWork();
        return await response.arrayBuffer();
    }

    if (options.skipCacheLookup !== true) {
        const hot = await readCachedTileArrayBuffer(cacheName, url);
        if (hot) return hot;
    }

    if (options.skipCacheLookup === true) {
        const data = await runInflightTileFetch(cacheName, url, async () => {
            await yieldBrowserTileWork();
            const response = await fetch(url);
            if (!response.ok) {
                if (response.status === 404 || response.status === 410) {
                    rememberMissingTile(cacheName, url, response.status);
                }
                throw new Error(`tile fetch failed: ${response.status}`);
            }
            await yieldBrowserTileWork();
            const data = await response.arrayBuffer();
            await yieldBrowserTileWork();
            writeTileMemoryCache(cacheName, url, data);
            forgetMissingTile(cacheName, url);
            if (data.byteLength <= TILE_CACHE_MAX_TILE_BYTES) {
                const headers = new Headers(response.headers);
                const cacheResponse = new Response(cloneTileArrayBuffer(data), {
                    status: response.status, statusText: response.statusText, headers,
                });
                Promise.resolve().then(async () => {
                    try {
                        await runBrowserTileCacheOp(async () => {
                            const cache = await openTileCache(cacheName);
                            if (cache) {
                                await cache.put(tileCacheRequestKey(url), cacheResponse);
                                bumpStoredTileCacheUsageBytes(data.byteLength);
                            }
                        });
                    } catch (e) {
                        if (mapDebugLoggingEnabled()) console.warn("[GS26 map] cache put failed", url, e);
                        schedulePersistentTileCacheUsageMeasure(0);
                    }
                });
            }
            return cloneTileArrayBuffer(data);
        });
        return cloneTileArrayBuffer(data);
    }

    const cache = await runBrowserTileCacheOp(async () => await openTileCache(cacheName));
    if (!cache) {
        await yieldBrowserTileWork();
        const response = await fetch(url);
        if (!response.ok) {
            if (response.status === 404 || response.status === 410) {
                rememberMissingTile(cacheName, url, response.status);
            }
            throw new Error(`tile fetch failed: ${response.status}`);
        }
        await yieldBrowserTileWork();
        return await response.arrayBuffer();
    }
    const cacheKey = tileCacheRequestKey(url);
    const cached = await runBrowserTileCacheOp(async () => await cache.match(cacheKey, {ignoreVary: true}));
    if (cached && cached.ok) {
        const data = await cached.arrayBuffer();
        writeTileMemoryCache(cacheName, url, data);
        forgetMissingTile(cacheName, url);
        return cloneTileArrayBuffer(data);
    }

    if (options.cacheOnly === true) {
        throw new Error("tile cache miss");
    }

    const data = await runInflightTileFetch(cacheName, url, async () => {
        await yieldBrowserTileWork();
        const response = await fetch(url);
        if (!response.ok) {
            if (response.status === 404 || response.status === 410) {
                rememberMissingTile(cacheName, url, response.status);
            }
            throw new Error(`tile fetch failed: ${response.status}`);
        }
        await yieldBrowserTileWork();
        const data = await response.arrayBuffer();
        await yieldBrowserTileWork();
        rememberTileSizeSample(data.byteLength);
        writeTileMemoryCache(cacheName, url, data);
        forgetMissingTile(cacheName, url);
        if (data.byteLength <= TILE_CACHE_MAX_TILE_BYTES) {
            const headers = new Headers(response.headers);
            const cacheResponse = new Response(cloneTileArrayBuffer(data), {
                status: response.status, statusText: response.statusText, headers,
            });
            Promise.resolve().then(async () => {
                try {
                    await runBrowserTileCacheOp(async () => {
                        await cache.put(cacheKey, cacheResponse);
                        bumpStoredTileCacheUsageBytes(data.byteLength);
                    });
                } catch (e) {
                    if (mapDebugLoggingEnabled()) console.warn("[GS26 map] cache put failed", url, e);
                    schedulePersistentTileCacheUsageMeasure(0);
                }
            });
        }
        return cloneTileArrayBuffer(data);
    });
    return cloneTileArrayBuffer(data);
}

async function prefetchTileToPersistentCache(cacheName, url) {
    if (!url) throw new Error("tile url missing");
    if (!tileFetchAllowedForUrl(url)) throw new Error(`unsupported tile url: ${url}`);
    if (!tileCacheEnabled() || !tileCacheSupported()) return;
    if (isKnownMissingTile(cacheName, url)) return;
    if (hasTileMemoryCache(cacheName, url)) return;

    const cache = await runBrowserTileCacheOp(async () => await openTileCache(cacheName));
    if (!cache) return;
    const cacheKey = tileCacheRequestKey(url);
    const cached = await runBrowserTileCacheOp(async () => await cache.match(cacheKey, {ignoreVary: true}));
    if (cached && cached.ok) {
        forgetMissingTile(cacheName, url);
        return;
    }

    await runInflightTileFetch(cacheName, url, async () => {
        await yieldBrowserTileWork();
        const response = await fetch(url);
        if (!response.ok) {
            if (response.status === 404 || response.status === 410) {
                rememberMissingTile(cacheName, url, response.status);
            }
            throw new Error(`tile fetch failed: ${response.status}`);
        }
        await yieldBrowserTileWork();
        const contentLengthRaw = response.headers ? response.headers.get("content-length") : null;
        const contentLength = contentLengthRaw == null ? NaN : Number(contentLengthRaw);
        rememberTileSizeSample(contentLength);
        if (Number.isFinite(contentLength) && contentLength > TILE_CACHE_MAX_TILE_BYTES) {
            return;
        }
        try {
            await runBrowserTileCacheOp(async () => {
                await cache.put(cacheKey, response);
            });
            forgetMissingTile(cacheName, url);
            if (Number.isFinite(contentLength) && contentLength >= 0) {
                bumpStoredTileCacheUsageBytes(contentLength);
            } else {
                schedulePersistentTileCacheUsageMeasure(0);
            }
        } catch (e) {
            if (mapDebugLoggingEnabled()) console.warn("[GS26 map] cache put failed", url, e);
            schedulePersistentTileCacheUsageMeasure(0);
        }
    });
}

function warmTileInBackground(cacheName, url) {
    Promise.resolve().then(async () => {
        try {
            await prefetchTileToPersistentCache(cacheName, url);
        } catch (e) {
        }
    });
}

function setTilePrefetchState(next) {
    const context = publishTilePrefetchContextState();
    tilePrefetchState = {
        ...tilePrefetchState,
        detail: "",
        userAvailable: context.userAvailable,
        rocketAvailable: context.rocketAvailable,
        userStatus: context.userStatus,
        rocketStatus: context.rocketStatus,
        contextStatus: context.summaryStatus,
        userMessage: context.userMessage,
        rocketMessage: context.rocketMessage,
        contextMessage: context.summaryMessage, ...next,
    };
    try {
        window.__gs26_ground_map_cache_state = {...tilePrefetchState};
        window.__gs26_ground_map_cache_ready = tilePrefetchState.state === "ready";
    } catch (e) {
    }
}

function buildTilePrefetchContextState() {
    const hasUser = Array.isArray(prefetchUserLatLng) && isUsableUserLatLng(prefetchUserLatLng[0], prefetchUserLatLng[1]);
    const hasRocket = Array.isArray(prefetchRocketLatLng) && isUsableLatLng(prefetchRocketLatLng[0], prefetchRocketLatLng[1]);
    let summaryStatus = "ready";
    let summaryMessage = "User and rocket prefetch are ready.";
    if (!hasUser && !hasRocket) {
        summaryStatus = "waiting";
        summaryMessage = "Waiting for user location and rocket telemetry.";
    } else if (!hasUser) {
        summaryStatus = "partial";
        summaryMessage = "User prefetch is deferred until location is available.";
    } else if (!hasRocket) {
        summaryStatus = "partial";
        summaryMessage = "Rocket prefetch is deferred until telemetry is available.";
    }
    return {
        userAvailable: hasUser,
        rocketAvailable: hasRocket,
        userStatus: hasUser ? "ready" : "waiting",
        rocketStatus: hasRocket ? "ready" : "waiting",
        summaryStatus,
        userMessage: hasUser ? "Ready" : "Waiting for user location.",
        rocketMessage: hasRocket ? "Ready" : "Waiting for rocket telemetry.",
        summaryMessage,
        updatedAt: Date.now(),
    };
}

function publishTilePrefetchContextState() {
    tilePrefetchContextState = buildTilePrefetchContextState();
    try {
        window.__gs26_ground_map_prefetch_context = {...tilePrefetchContextState};
    } catch (e) {
    }
    return tilePrefetchContextState;
}

function emptyTilePrefetchPlan() {
    return {
        key: "", coords: [], breakdown: {
            userTiles: 0, rocketTiles: 0, combinedTiles: 0,
        },
    };
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
    const h = Math.sin(dLat / 2.0) ** 2 + Math.cos(p1) * Math.cos(p2) * Math.sin(dLon / 2.0) ** 2;
    return 2.0 * r * Math.atan2(Math.sqrt(h), Math.sqrt(Math.max(0.0, 1.0 - h)));
}

function isUsableLatLng(lat, lon) {
    const latNum = Number(lat);
    const lonNum = Number(lon);
    if (!Number.isFinite(latNum) || !Number.isFinite(lonNum)) return false;
    if (Math.abs(latNum) > 85.05112878 || Math.abs(lonNum) > 180.0) return false;
    return true;
}

function isUsableUserLatLng(lat, lon) {
    if (!isUsableLatLng(lat, lon)) return false;
    const latNum = Number(lat);
    const lonNum = Number(lon);
    if (Math.abs(latNum) < 0.000001 && Math.abs(lonNum) < 0.000001) return false;
    return true;
}

function stabilizeLatLng(stateName, lat, lon) {
    const state = stateName === "rocket" ? rocketGpsStability : userGpsStability;
    const nextState = state || {
        candidate: null, count: 0, accepted: false, acceptedLatLng: null,
    };
    const isUsable = stateName === "user" ? isUsableUserLatLng(lat, lon) : isUsableLatLng(lat, lon);
    if (!isUsable) {
        nextState.candidate = null;
        nextState.count = 0;
        if (stateName === "rocket") rocketGpsStability = nextState; else userGpsStability = nextState;
        return nextState.accepted ? nextState.acceptedLatLng : null;
    }

    const next = [Number(lat), Number(lon)];
    if (nextState.accepted) {
        nextState.acceptedLatLng = next;
        nextState.candidate = next;
        nextState.count = GPS_STABLE_FIX_REQUIRED;
        if (stateName === "rocket") rocketGpsStability = nextState; else userGpsStability = nextState;
        return next;
    }

    const acceptedDistanceM = Array.isArray(nextState.acceptedLatLng) ? distanceMetersBetween(nextState.acceptedLatLng, next) : Infinity;
    if (nextState.accepted && Number.isFinite(acceptedDistanceM) && acceptedDistanceM <= GPS_STABLE_DISTANCE_M) {
        nextState.acceptedLatLng = next;
        nextState.candidate = next;
        nextState.count = GPS_STABLE_FIX_REQUIRED;
        if (stateName === "rocket") rocketGpsStability = nextState; else userGpsStability = nextState;
        return next;
    }

    const candidateDistanceM = Array.isArray(nextState.candidate) ? distanceMetersBetween(nextState.candidate, next) : Infinity;
    if (Number.isFinite(candidateDistanceM) && candidateDistanceM <= GPS_STABLE_DISTANCE_M) {
        nextState.count += 1;
        nextState.candidate = next;
    } else {
        nextState.candidate = next;
        nextState.count = 1;
    }

    if (nextState.count >= GPS_STABLE_FIX_REQUIRED) {
        nextState.accepted = true;
        nextState.acceptedLatLng = next;
        if (stateName === "rocket") rocketGpsStability = nextState; else userGpsStability = nextState;
        return next;
    }

    if (stateName === "rocket") rocketGpsStability = nextState; else userGpsStability = nextState;
    return nextState.accepted ? nextState.acceptedLatLng : null;
}

function latLngOffsetMeters(fromLatLng, toLatLng) {
    if (!Array.isArray(fromLatLng) || !Array.isArray(toLatLng)) return null;
    const fromLat = Number(fromLatLng[0]);
    const fromLon = Number(fromLatLng[1]);
    const toLat = Number(toLatLng[0]);
    const toLon = Number(toLatLng[1]);
    if (![fromLat, fromLon, toLat, toLon].every(Number.isFinite)) return null;
    let lonDiff = clampLon(toLon) - clampLon(fromLon);
    if (lonDiff > 180.0) lonDiff -= 360.0;
    if (lonDiff < -180.0) lonDiff += 360.0;
    const midLat = (fromLat + toLat) * 0.5;
    return {
        north: (toLat - fromLat) * metersPerDegreeLat(), east: lonDiff * metersPerDegreeLon(midLat),
    };
}

function latLngFromOffsetMeters(fromLatLng, northM, eastM) {
    if (!Array.isArray(fromLatLng)) return null;
    const lat = Number(fromLatLng[0]);
    const lon = Number(fromLatLng[1]);
    if (![lat, lon, northM, eastM].every(Number.isFinite)) return null;
    const nextLat = clampLat(lat + northM / metersPerDegreeLat());
    const nextLon = clampLon(lon + eastM / metersPerDegreeLon((lat + nextLat) * 0.5));
    return [nextLat, nextLon];
}

function emptyFeatureCollection() {
    return {
        type: "FeatureCollection", features: [],
    };
}

function pointFeatureCollection(latLng) {
    if (!Array.isArray(latLng)) return emptyFeatureCollection();
    return {
        type: "FeatureCollection", features: [{
            type: "Feature", geometry: {
                type: "Point", coordinates: [latLng[1], latLng[0]],
            }, properties: {},
        }],
    };
}

function headingFeatureCollection(latLng, headingDeg) {
    if (!Array.isArray(latLng)) return emptyFeatureCollection();
    const resolvedHeadingDeg = Number.isFinite(headingDeg) ? normalizeAngle(headingDeg) : 0;
    return {
        type: "FeatureCollection", features: [{
            type: "Feature", geometry: {
                type: "Point", coordinates: [latLng[1], latLng[0]],
            }, properties: {bearing: resolvedHeadingDeg},
        }],
    };
}

function blendLatLngToward(fromLatLng, toLatLng, alpha) {
    if (!Array.isArray(fromLatLng) || !Array.isArray(toLatLng)) return Array.isArray(toLatLng) ? [toLatLng[0], toLatLng[1]] : null;
    const clampedAlpha = Math.max(0.0, Math.min(1.0, Number(alpha) || 0));
    const fromLon = clampLon(fromLatLng[1]);
    let lonDiff = clampLon(toLatLng[1]) - fromLon;
    if (lonDiff > 180.0) lonDiff -= 360.0;
    if (lonDiff < -180.0) lonDiff += 360.0;
    return [clampLat(fromLatLng[0] + (toLatLng[0] - fromLatLng[0]) * clampedAlpha), clampLon(fromLon + lonDiff * clampedAlpha),];
}

function speedLimitedLatLngToward(fromLatLng, toLatLng, maxDistanceM) {
    const distanceM = distanceMetersBetween(fromLatLng, toLatLng);
    if (!Number.isFinite(distanceM)) return Array.isArray(toLatLng) ? [toLatLng[0], toLatLng[1]] : null;
    if (distanceM <= 0.001) return [toLatLng[0], toLatLng[1]];
    const allowedM = Math.max(0.0, Number(maxDistanceM) || 0.0);
    if (allowedM >= distanceM) return [toLatLng[0], toLatLng[1]];
    return blendLatLngToward(fromLatLng, toLatLng, allowedM / distanceM);
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
    const y = Math.floor(((1.0 - Math.log(Math.tan(latRad) + 1.0 / Math.cos(latRad)) / Math.PI) / 2.0) * scale);
    return {
        x: Math.max(0, Math.min(scale - 1, x)), y: Math.max(0, Math.min(scale - 1, y)),
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
    return tileCoordsForRange(nw, se, zoom, HIGH_RES_PREFETCH_LOCATION_BUFFER_TILES);
}

function tileCoordsForRange(nw, se, zoom, bufferTiles = 0) {
    const coords = [];
    const z = Math.max(effectiveMinZoom(), Math.floor(Number(zoom)));
    if (!Number.isFinite(z)) return coords;
    const scale = Math.pow(2, z);
    if (!Number.isFinite(scale) || scale < 1) return coords;
    const buffer = Math.max(0, Math.floor(Number(bufferTiles) || 0));
    const xMin = Math.max(0, Math.min(nw.x, se.x) - buffer);
    const xMax = Math.min(scale - 1, Math.max(nw.x, se.x) + buffer);
    const yMin = Math.max(0, Math.min(nw.y, se.y) - buffer);
    const yMax = Math.min(scale - 1, Math.max(nw.y, se.y) + buffer);
    for (let x = xMin; x <= xMax; x++) {
        for (let y = yMin; y <= yMax; y++) {
            coords.push({x, y, z});
        }
    }
    return coords;
}

function tileRangeForBounds(bounds, zoom, bufferTiles = 0) {
    if (!bounds) return null;
    const north = bounds.getNorth();
    const south = bounds.getSouth();
    const east = bounds.getEast();
    const west = bounds.getWest();
    const nw = latLonToTileXY(north, west, zoom);
    const se = latLonToTileXY(south, east, zoom);
    return tileRangeForCorners(nw, se, zoom, bufferTiles);
}

function tileRangeForRadius(lat, lon, zoom, radiusM) {
    const dLat = radiusM / metersPerDegreeLat();
    const dLon = radiusM / metersPerDegreeLon(lat);
    const north = lat + dLat;
    const south = lat - dLat;
    const east = lon + dLon;
    const west = lon - dLon;
    const nw = latLonToTileXY(north, west, zoom);
    const se = latLonToTileXY(south, east, zoom);
    return tileRangeForCorners(nw, se, zoom, HIGH_RES_PREFETCH_LOCATION_BUFFER_TILES);
}

function tileRangeForCorners(nw, se, zoom, bufferTiles = 0) {
    const z = Math.max(effectiveMinZoom(), Math.floor(Number(zoom)));
    if (!Number.isFinite(z)) return null;
    const scale = Math.pow(2, z);
    if (!Number.isFinite(scale) || scale < 1) return null;
    const buffer = Math.max(0, Math.floor(Number(bufferTiles) || 0));
    const xMin = Math.max(0, Math.min(nw.x, se.x) - buffer);
    const xMax = Math.min(scale - 1, Math.max(nw.x, se.x) + buffer);
    const yMin = Math.max(0, Math.min(nw.y, se.y) - buffer);
    const yMax = Math.min(scale - 1, Math.max(nw.y, se.y) + buffer);
    return xMax >= xMin && yMax >= yMin ? {z, xMin, xMax, yMin, yMax} : null;
}

function tileCountForRange(range) {
    if (!range) return 0;
    return Math.max(0, (range.xMax - range.xMin + 1) * (range.yMax - range.yMin + 1));
}

function tileCoordsForBounds(bounds, zoom) {
    if (!bounds) return [];
    const north = bounds.getNorth();
    const south = bounds.getSouth();
    const east = bounds.getEast();
    const west = bounds.getWest();
    const nw = latLonToTileXY(north, west, zoom);
    const se = latLonToTileXY(south, east, zoom);
    return tileCoordsForRange(nw, se, zoom, HIGH_RES_PREFETCH_VIEWPORT_BUFFER_TILES);
}

function tileCoordsAroundTileRadius(lat, lon, zoom, radiusTiles) {
    const center = latLonToTileXY(lat, lon, zoom);
    return tileCoordsForRange(center, center, zoom, radiusTiles);
}

function tileCoordsForViewportAround(lat, lon, zoom, bufferTiles = 0) {
    const z = Math.max(effectiveMinZoom(), Math.floor(Number(zoom)));
    const centerLat = Number(lat);
    const centerLon = Number(lon);
    if (!Number.isFinite(z) || !Number.isFinite(centerLat) || !Number.isFinite(centerLon)) return [];
    const container = groundMap && typeof groundMap.getContainer === "function" ? groundMap.getContainer() : null;
    const width = Math.max(320, Number(container && container.clientWidth) || window.innerWidth || 800);
    const height = Math.max(320, Number(container && container.clientHeight) || window.innerHeight || 600);
    const centerPoint = latLonToWorldPointAtZoom(centerLat, centerLon, z);
    const coverPx = Math.ceil(Math.hypot(width, height) / 2) + Math.max(0, Math.floor(Number(bufferTiles) || 0)) * 256;
    const nw = {
        x: Math.floor((centerPoint.x - coverPx) / 256), y: Math.floor((centerPoint.y - coverPx) / 256),
    };
    const se = {
        x: Math.ceil((centerPoint.x + coverPx) / 256), y: Math.ceil((centerPoint.y + coverPx) / 256),
    };
    return tileCoordsForRange(nw, se, z, 0);
}

function prefetchZoomLevels(maxNativeZoom, focusZoom) {
    const top = Math.max(MIN_ZOOM, Math.floor(Number(maxNativeZoom) || DEFAULT_MAX_NATIVE_ZOOM));
    const bottom = effectiveMinZoom();
    const zooms = [];
    const seen = new Set();
    const add = (value) => {
        const z = Math.max(bottom, Math.min(top, Math.floor(Number(value))));
        if (!Number.isFinite(z) || seen.has(z)) return;
        seen.add(z);
        zooms.push(z);
    };

    if (Number.isFinite(focusZoom)) {
        const center = Math.floor(focusZoom);
        add(center);
        for (let delta = 1; delta <= HIGH_RES_PREFETCH_FOCUS_ZOOM_DELTA; delta++) {
            add(center + delta);
            add(center - delta);
        }
    }
    for (let z = top; z >= bottom; z--) {
        add(z);
    }
    return zooms;
}

function trackingZoomLevels(maxNativeZoom, focusZoom) {
    const top = Math.max(MIN_ZOOM, Math.floor(Number(maxNativeZoom) || DEFAULT_MAX_NATIVE_ZOOM));
    const bottom = effectiveMinZoom();
    const center = Math.max(bottom, Math.min(top, Math.floor(Number.isFinite(focusZoom) ? focusZoom : top)));
    const zooms = [];
    const seen = new Set();
    const add = (value) => {
        const z = Math.max(bottom, Math.min(top, Math.floor(Number(value))));
        if (!Number.isFinite(z) || seen.has(z)) return;
        seen.add(z);
        zooms.push(z);
    };
    add(center);
    for (let delta = 1; delta <= TRACKING_PREFETCH_ZOOM_DELTA; delta++) {
        add(center + delta);
        add(center - delta);
    }
    return zooms;
}

function appendUniqueCoords(target, seen, coords, maxTiles) {
    const hasLimit = Number.isFinite(Number(maxTiles)) && Number(maxTiles) > 0;
    const limit = hasLimit ? Number(maxTiles) : Infinity;
    for (const coord of coords) {
        const id = `${coord.z}/${coord.x}/${coord.y}`;
        if (seen.has(id)) continue;
        seen.add(id);
        target.push(coord);
        if (target.length >= limit) break;
    }
}

function clampMaxNativeZoom(value) {
    if (!Number.isFinite(value)) return DEFAULT_MAX_NATIVE_ZOOM;
    return Math.max(DEFAULT_SAFE_MIN_ZOOM, Math.min(MAX_NATIVE_ZOOM_LIMIT, Math.floor(value)));
}

function clampMaxDisplayZoom(value, maxNativeZoom) {
    const nativeZoom = clampMaxNativeZoom(maxNativeZoom);
    const requested = Math.floor(Number(value));
    if (!Number.isFinite(requested)) {
        return Math.min(MAX_DISPLAY_ZOOM_LIMIT, nativeZoom + DEFAULT_MAX_OVERZOOM_DELTA);
    }
    return Math.max(nativeZoom + DEFAULT_MAX_OVERZOOM_DELTA, Math.min(MAX_DISPLAY_ZOOM_LIMIT, requested));
}

function promoteNativeZoomForDisplayZoom(displayZoom, tilesUrl = currentTilesUrl) {
    const value = Number(displayZoom);
    if (!Number.isFinite(value)) return false;
    const targetNativeZoom = clampMaxNativeZoom(Math.floor(value));
    if (Number.isFinite(currentMaxNativeZoom) && currentMaxNativeZoom >= targetNativeZoom) {
        return false;
    }
    currentMaxNativeZoom = targetNativeZoom;
    currentMaxZoom = Math.max(Number.isFinite(currentMaxZoom) ? currentMaxZoom : DEFAULT_SAFE_MIN_ZOOM, Math.min(MAX_DISPLAY_ZOOM_LIMIT, currentMaxNativeZoom + DEFAULT_MAX_OVERZOOM_DELTA));
    try {
        window.__gs26_max_native_zoom = currentMaxNativeZoom;
    } catch (e) {
    }
    persistMaxNativeZoom(tilesUrl, currentMaxNativeZoom);
    return true;
}

function tileZoomCacheKey(tilesUrl) {
    return canonicalTileSourceKey(tilesUrl);
}

function loadPersistedMaxNativeZoom(tilesUrl) {
    const key = tileZoomCacheKey(tilesUrl);
    if (!key) return null;
    try {
        const storage = window.localStorage || null;
        const raw = (storage ? storage.getItem(MAP_MAX_ZOOM_STORAGE_KEY) : null) || window.__gs26_ground_map_max_zoom_json;
        if (!raw) return null;
        const parsed = JSON.parse(raw);
        const value = Number(parsed && parsed[key] && parsed[key].maxNativeZoom);
        if (!Number.isFinite(value)) return null;
        return clampMaxNativeZoom(value);
    } catch (e) {
        return null;
    }
}

function loadPersistedMinNativeZoom(tilesUrl) {
    const key = tileZoomCacheKey(tilesUrl);
    if (!key) return null;
    try {
        const storage = window.localStorage || null;
        const raw = (storage ? storage.getItem(MAP_MAX_ZOOM_STORAGE_KEY) : null) || window.__gs26_ground_map_max_zoom_json;
        if (!raw) return null;
        const parsed = JSON.parse(raw);
        const value = Number(parsed && parsed[key] && parsed[key].minNativeZoom);
        if (!Number.isFinite(value)) return null;
        return Math.max(DEFAULT_SAFE_MIN_ZOOM, Math.floor(value));
    } catch (e) {
        return null;
    }
}

function persistTileZoomRange(tilesUrl, minNativeZoom, maxNativeZoom) {
    if (!tileCacheEnabled()) return;
    const key = tileZoomCacheKey(tilesUrl);
    const maxValue = clampMaxNativeZoom(Number(maxNativeZoom));
    const minValue = Math.max(DEFAULT_SAFE_MIN_ZOOM, Math.min(maxValue, Math.floor(Number(minNativeZoom))));
    if (!key || !Number.isFinite(maxValue) || !Number.isFinite(minValue)) return;
    try {
        const storage = window.localStorage || null;
        const raw = (storage ? storage.getItem(MAP_MAX_ZOOM_STORAGE_KEY) : null) || window.__gs26_ground_map_max_zoom_json;
        let parsed = {};
        if (raw) {
            parsed = JSON.parse(raw) || {};
        }
        parsed[key] = {
            minNativeZoom: minValue, maxNativeZoom: maxValue, updatedAt: Date.now(),
        };
        const nextRaw = JSON.stringify(parsed);
        window.__gs26_ground_map_max_zoom_json = nextRaw;
        if (storage) {
            storage.setItem(MAP_MAX_ZOOM_STORAGE_KEY, nextRaw);
        }
    } catch (e) {
    }
}

function persistMaxNativeZoom(tilesUrl, maxNativeZoom) {
    const minZoom = loadPersistedMinNativeZoom(tilesUrl);
    persistTileZoomRange(tilesUrl, Number.isFinite(minZoom) ? minZoom : MIN_ZOOM, maxNativeZoom);
}

function cachedRequestUrlToOriginal(rawUrl) {
    const value = String(rawUrl || "");
    const prefix = "https://gs26.tile-cache.local/";
    if (value.startsWith(prefix)) {
        try {
            return decodeURIComponent(value.slice(prefix.length));
        } catch (e) {
            return value;
        }
    }
    return value;
}

function tileUrlZoomRegex(tilesUrl) {
    const template = String(tilesUrl || "");
    if (!template.includes("{z}") || !template.includes("{x}") || !template.includes("{y}")) return null;
    const escaped = template
        .replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
        .replace("\\{z\\}", "(\\d+)")
        .replace("\\{x\\}", "\\d+")
        .replace("\\{y\\}", "\\d+");
    return new RegExp(`^${escaped}$`);
}

async function discoverCachedNativeZoomRange(tilesUrl) {
    if (!tileCacheSupported()) return null;
    const key = tileZoomCacheKey(tilesUrl);
    if (!key) return null;
    const matcher = tileUrlZoomRegex(tilesUrl);
    if (!matcher) return null;
    try {
        const cache = await caches.open(tileCacheName(tilesUrl));
        const requests = await cache.keys();
        let minZoom = null;
        let maxZoom = null;
        for (const request of requests) {
            const raw = request && request.url ? request.url : "";
            const original = cachedRequestUrlToOriginal(raw);
            const match = original.match(matcher);
            if (!match) continue;
            const zoom = Number(match[1]);
            if (Number.isFinite(zoom)) {
                minZoom = minZoom == null ? zoom : Math.min(minZoom, zoom);
                maxZoom = maxZoom == null ? zoom : Math.max(maxZoom, zoom);
            }
        }
        return Number.isFinite(maxZoom) ? {
            minNativeZoom: Math.max(MIN_ZOOM, Math.floor(minZoom)), maxNativeZoom: clampMaxNativeZoom(maxZoom),
        } : null;
    } catch (e) {
        return null;
    }
}

async function discoverCachedMaxNativeZoom(tilesUrl) {
    const range = await discoverCachedNativeZoomRange(tilesUrl);
    return range ? range.maxNativeZoom : null;
}

function applyDiscoveredCachedMaxNativeZoom(tilesUrl) {
    if (!tileCacheEnabled()) return;
    discoverCachedNativeZoomRange(tilesUrl).then((range) => {
        if (!range || effectivePrefetchTilesUrl() !== tilesUrl) return;
        const zoom = range.maxNativeZoom;
        if (Number.isFinite(currentMaxNativeZoom) && zoom <= currentMaxNativeZoom) {
            persistTileZoomRange(tilesUrl, Number.isFinite(currentMinZoom) ? currentMinZoom : MIN_ZOOM, currentMaxNativeZoom);
            restorePendingZoomIfPossible();
            return;
        }
        currentMaxNativeZoom = clampMaxNativeZoom(zoom);
        currentMaxZoom = Math.max(currentMaxZoom || 0, currentMaxNativeZoom + DEFAULT_MAX_OVERZOOM_DELTA);
        persistTileZoomRange(tilesUrl, Number.isFinite(currentMinZoom) ? currentMinZoom : MIN_ZOOM, currentMaxNativeZoom);
        try {
            if (groundMap) {
                groundMap.setMaxZoom(currentMaxZoom);
                updateZoomControlAppearance();
                restorePendingZoomIfPossible();
            }
        } catch (e) {
        }
    }).catch(() => {
    });
}

function scheduleCachedZoomDiscoveryAfterStartup(tilesUrl) {
    if (!tileCacheEnabled() || !tilesUrl) return;
    if (Number.isFinite(loadPersistedMaxNativeZoom(tilesUrl))) return;
    idleDelay(safeMapCallback("startup cached zoom discovery", () => {
        if (currentTilesUrl === tilesUrl) {
            applyDiscoveredCachedMaxNativeZoom(tilesUrl);
        }
    }), STARTUP_ZOOM_DISCOVERY_DELAY_MS + 2500);
}

function refreshRasterTileSourceForZoom() {
    if (!groundMap || !currentTilesUrl) return;
    try {
        if (groundMap.getLayer && groundMap.getLayer(TILE_LAYER_ID)) {
            groundMap.removeLayer(TILE_LAYER_ID);
        }
        if (groundMap.getSource && groundMap.getSource(TILE_SOURCE_ID)) {
            groundMap.removeSource(TILE_SOURCE_ID);
        }
        const rasterTemplate = shouldUseNativeTileTemplate(currentTilesUrl) ? String(currentTilesUrl || "") : tileProtocolTemplate();
        groundMap.addSource(TILE_SOURCE_ID, {
            type: "raster",
            tiles: [rasterTemplate],
            tileSize: 256,
            bounds: [NA_BOUNDS.lonMin, NA_BOUNDS.latMin, NA_BOUNDS.lonMax, NA_BOUNDS.latMax],
            minzoom: effectiveMinZoom(),
            maxzoom: Math.max(effectiveMinZoom(), MAX_NATIVE_ZOOM_LIMIT),
        });
        const beforeLayer = groundMap.getLayer && groundMap.getLayer(GUIDE_LAYER_ID) ? GUIDE_LAYER_ID : undefined;
        groundMap.addLayer({
            id: TILE_LAYER_ID, type: "raster", source: TILE_SOURCE_ID, paint: {
                "raster-opacity": 1,
            },
        }, beforeLayer);
    } catch (e) {
    }
}

function loadPersistedMapState() {
    try {
        const storage = window.localStorage || null;
        const raw = (storage ? storage.getItem(MAP_STATE_STORAGE_KEY) : null) || window.__gs26_ground_map_state_json;
        if (!raw) return;
        const parsed = JSON.parse(raw);
        if (Number.isFinite(parsed.zoom)) {
            lastMapZoom = parsed.zoom;
            pendingRestoreZoom = parsed.zoom;
        }
        if (Number.isFinite(parsed.lat) && Number.isFinite(parsed.lon)) {
            lastMapView = {
                lat: parsed.lat, lon: parsed.lon, zoom: Number.isFinite(parsed.zoom) ? parsed.zoom : null,
            };
        }
        if (parsed.orientationMode === "manual" || parsed.orientationMode === "user" || parsed.orientationMode === "north") {
            orientationMode = parsed.orientationMode;
        }
        if (typeof parsed.followUserEnabled === "boolean") {
            followUserEnabled = parsed.followUserEnabled;
        }
        if (Number.isFinite(parsed.bearingDeg)) {
            mapBearingDeg = normalizeAngle(parsed.bearingDeg);
        }
        if (isUsableUserLatLng(parsed.userLat, parsed.userLon)) {
            lastUserLatLng = [Number(parsed.userLat), Number(parsed.userLon)];
            userMarkerDisplayedLatLng = [lastUserLatLng[0], lastUserLatLng[1]];
            userMarkerHasLiveFix = true;
            userGpsStability = {
                candidate: lastUserLatLng,
                count: GPS_STABLE_FIX_REQUIRED,
                accepted: true,
                acceptedLatLng: lastUserLatLng,
            };
        } else {
            lastUserLatLng = null;
            userMarkerDisplayedLatLng = null;
            userMarkerHasLiveFix = false;
            userGpsStability = null;
            try {
                delete window.__gs26_user_lat;
                delete window.__gs26_user_lon;
            } catch (e) {
                window.__gs26_user_lat = NaN;
                window.__gs26_user_lon = NaN;
            }
        }
    } catch (e) {
    }
}

function restorePendingZoomIfPossible() {
    if (!groundMap || !Number.isFinite(pendingRestoreZoom) || !Number.isFinite(currentMaxZoom)) {
        return false;
    }
    const targetZoom = Math.max(effectiveMinZoom(), Math.min(currentMaxZoom, pendingRestoreZoom));
    if (targetZoom + WHEEL_ZOOM_LIMIT_EPSILON < pendingRestoreZoom) {
        return false;
    }
    const currentZoom = Number(groundMap.getZoom && groundMap.getZoom());
    pendingRestoreZoom = null;
    lastMapZoom = targetZoom;
    if (Number.isFinite(currentZoom) && Math.abs(currentZoom - targetZoom) <= WHEEL_ZOOM_LIMIT_EPSILON) {
        rememberMapView();
        persistMapStateSoon();
        updateZoomControlAppearance();
        return true;
    }
    try {
        if (typeof groundMap.stop === "function") {
            groundMap.stop();
        }
    } catch (e) {
    }
    markInternalCameraUpdate(120);
    groundMap.jumpTo({
        zoom: targetZoom, bearing: mapBearingDeg,
    });
    rememberMapView();
    persistMapStateSoon();
    updateZoomControlAppearance();
    return true;
}

function persistMapState() {
    try {
        const storage = window.localStorage || null;
        const payload = {
            lat: lastMapView && Number.isFinite(lastMapView.lat) ? lastMapView.lat : null,
            lon: lastMapView && Number.isFinite(lastMapView.lon) ? lastMapView.lon : null,
            zoom: Number.isFinite(lastMapZoom) ? lastMapZoom : (lastMapView && Number.isFinite(lastMapView.zoom) ? lastMapView.zoom : null),
            orientationMode,
            followUserEnabled,
            bearingDeg: mapBearingDeg,
            userLat: Array.isArray(lastUserLatLng) && isUsableUserLatLng(lastUserLatLng[0], lastUserLatLng[1]) ? lastUserLatLng[0] : null,
            userLon: Array.isArray(lastUserLatLng) && isUsableUserLatLng(lastUserLatLng[0], lastUserLatLng[1]) ? lastUserLatLng[1] : null,
        };
        const raw = JSON.stringify(payload);
        window.__gs26_ground_map_state_json = raw;
        if (storage) {
            storage.setItem(MAP_STATE_STORAGE_KEY, raw);
        }
    } catch (e) {
    }
}

function persistMapStateSoon() {
    const now = Date.now();
    if (now - lastPersistedMapStateAtMs < 1000) return;
    lastPersistedMapStateAtMs = now;
    persistMapState();
}

loadPersistedMapState();

function rememberMapView() {
    if (!groundMap) return;
    const center = groundMap.getCenter();
    const zoom = groundMap.getZoom();
    const preservePendingZoom = Number.isFinite(pendingRestoreZoom) && pendingRestoreZoom > zoom && Number.isFinite(currentMaxZoom) && zoom >= currentMaxZoom - 0.001;
    if (!preservePendingZoom) {
        lastMapZoom = zoom;
        pendingRestoreZoom = null;
    }
    lastMapView = {
        lat: center.lat, lon: center.lng, zoom: preservePendingZoom ? pendingRestoreZoom : zoom,
    };
    mapBearingDeg = normalizeAngle(groundMap.getBearing());
    syncWindowMapControlState();
}

function pointDataKey(latLng) {
    if (!Array.isArray(latLng)) return SOURCE_EMPTY_KEY;
    return `${Number(latLng[0]).toFixed(7)},${Number(latLng[1]).toFixed(7)}`;
}

function guideLineDataKey(rocketLatLng, userLatLng) {
    if (!Array.isArray(rocketLatLng) || !Array.isArray(userLatLng)) return SOURCE_EMPTY_KEY;
    return `${pointDataKey(userLatLng)}|${pointDataKey(rocketLatLng)}`;
}

function currentUserVisualOrLastLatLng() {
    return currentUserMarkerVisualLatLng() || userMarkerDisplayedLatLng || lastUserLatLng;
}

function headingDataKey(latLng, bearingDeg) {
    if (!Array.isArray(latLng) || !Number.isFinite(bearingDeg)) return SOURCE_EMPTY_KEY;
    return `${pointDataKey(latLng)}|${normalizeAngle(bearingDeg).toFixed(1)}`;
}

function syncWindowMapControlState() {
    try {
        window.__gs26_follow_user_enabled = followUserEnabled ? "true" : "false";
        window.__gs26_follow_user_enable_guard_until = followEnableGuardUntilMs;
        window.__gs26_map_orientation_mode = orientationMode;
        window.__gs26_map_bearing_deg = mapBearingDeg;
    } catch (e) {
    }
}

function effectiveMaxNativeZoomFor(configMaxNativeZoom, tilesUrl) {
    const configured = clampMaxNativeZoom(configMaxNativeZoom);
    if (!tileCacheEnabled()) return configured;
    const cached = loadPersistedMaxNativeZoom(tilesUrl || currentTilesUrl);
    if (Number.isFinite(cached)) {
        return Math.max(configured, cached);
    }
    return configured;
}

function tileRangeKeyForBounds(bounds, zoom, bufferTiles = 0) {
    if (!bounds) return "";
    const z = Math.max(effectiveMinZoom(), Math.floor(Number(zoom) || effectiveMinZoom()));
    const nw = latLonToTileXY(bounds.getNorth(), bounds.getWest(), z);
    const se = latLonToTileXY(bounds.getSouth(), bounds.getEast(), z);
    const scale = Math.pow(2, z);
    const buffer = Math.max(0, Math.floor(Number(bufferTiles) || 0));
    const xMin = Math.max(0, Math.min(nw.x, se.x) - buffer);
    const xMax = Math.min(scale - 1, Math.max(nw.x, se.x) + buffer);
    const yMin = Math.max(0, Math.min(nw.y, se.y) - buffer);
    const yMax = Math.min(scale - 1, Math.max(nw.y, se.y) + buffer);
    return `${z}:${xMin}:${yMin}:${xMax}:${yMax}`;
}

function scheduleTileZoomDiscovery() {
    return;
}

function buildTrackingPrefetchPlan() {
    const tilesUrl = effectivePrefetchTilesUrl();
    if (!tilesUrl) {
        return {key: "", coords: [], breakdown: {userTiles: 0, rocketTiles: 0, combinedTiles: 0}};
    }

    const userLat = Array.isArray(prefetchUserLatLng) ? Number(prefetchUserLatLng[0]) : NaN;
    const userLon = Array.isArray(prefetchUserLatLng) ? Number(prefetchUserLatLng[1]) : NaN;
    const hasUser = isUsableUserLatLng(userLat, userLon);
    if (!hasUser) {
        return {key: "", coords: [], breakdown: {userTiles: 0, rocketTiles: 0, combinedTiles: 0}};
    }

    const maxNativeZoom = effectivePrefetchMaxNativeZoom(tilesUrl);
    const focusZoom = groundMap && typeof groundMap.getZoom === "function" ? groundMap.getZoom() : (lastMapView && Number.isFinite(lastMapView.zoom) ? lastMapView.zoom : maxNativeZoom);
    const zooms = trackingZoomLevels(maxNativeZoom, focusZoom);
    const focusZoomInt = Math.max(effectiveMinZoom(), Math.min(maxNativeZoom, Math.floor(focusZoom)));
    const userFocusTile = latLonToTileXY(userLat, userLon, focusZoomInt);
    const container = groundMap && typeof groundMap.getContainer === "function" ? groundMap.getContainer() : null;
    const key = ["tracking-user", tilesUrl, String(maxNativeZoom), Number.isFinite(focusZoom) ? Math.floor(focusZoom).toString() : "", userFocusTile.x, userFocusTile.y, container ? Math.round(container.clientWidth || 0) : "", container ? Math.round(container.clientHeight || 0) : "",].join("|");

    const coords = [];
    const seen = new Set();
    const maxTiles = Number.POSITIVE_INFINITY;
    const viewportBaseZoom = Math.max(effectiveMinZoom(), Math.min(maxNativeZoom, Math.floor(Number.isFinite(focusZoom) ? focusZoom : maxNativeZoom)));
    for (let delta = -TRACKING_PREFETCH_ZOOM_IN_VIEWPORT_LEVELS; delta <= TRACKING_PREFETCH_ZOOM_OUT_VIEWPORT_LEVELS; delta++) {
        const zoom = Math.max(effectiveMinZoom(), Math.min(maxNativeZoom, viewportBaseZoom + delta));
        appendUniqueCoords(coords, seen, tileCoordsForViewportAround(userLat, userLon, zoom, TRACKING_PREFETCH_VIEWPORT_BUFFER_TILES), maxTiles);
    }
    for (const zoom of zooms) {
        appendUniqueCoords(coords, seen, tileCoordsAroundTileRadius(userLat, userLon, zoom, TRACKING_PREFETCH_TILE_RADIUS), maxTiles);
    }

    return {
        key, coords, breakdown: {
            userTiles: coords.length, rocketTiles: 0, combinedTiles: coords.length,
        },
    };
}

function buildHighResPrefetchPlan() {
    const tilesUrl = effectivePrefetchTilesUrl();
    if (!tilesUrl) {
        return {key: "", coords: [], breakdown: {userTiles: 0, rocketTiles: 0, combinedTiles: 0}};
    }

    const nativeDesktop = isDesktopNativeMapRuntime();
    const maxNativeZoom = effectivePrefetchMaxNativeZoom(tilesUrl);
    const mapZoom = groundMap && typeof groundMap.getZoom === "function" ? groundMap.getZoom() : (lastMapView && Number.isFinite(lastMapView.zoom) ? lastMapView.zoom : NaN);
    const zooms = prefetchZoomLevels(maxNativeZoom, mapZoom);
    const coords = [];
    const seen = new Set();
    const userCoords = [];
    const userSeen = new Set();
    const rocketCoords = [];
    const rocketSeen = new Set();
    const userLat = Array.isArray(prefetchUserLatLng) ? prefetchUserLatLng[0] : NaN;
    const userLon = Array.isArray(prefetchUserLatLng) ? prefetchUserLatLng[1] : NaN;
    const rocketLat = Array.isArray(prefetchRocketLatLng) ? prefetchRocketLatLng[0] : NaN;
    const rocketLon = Array.isArray(prefetchRocketLatLng) ? prefetchRocketLatLng[1] : NaN;
    const hasUser = isUsableUserLatLng(userLat, userLon);
    const hasRocket = isUsableLatLng(rocketLat, rocketLon);
    const userRadiusM = configuredPrefetchRadiusM("user");
    const rocketRadiusM = configuredPrefetchRadiusM("rocket");
    if (!hasUser && !hasRocket) {
        return {key: "", coords: [], breakdown: {userTiles: 0, rocketTiles: 0, combinedTiles: 0}};
    }
    const bounds = groundMap && groundMap.getBounds ? groundMap.getBounds() : null;
    const viewportZoom = Math.max(effectiveMinZoom(), Math.min(maxNativeZoom, Math.floor(Number.isFinite(mapZoom) ? mapZoom : maxNativeZoom)));
    const viewportKey = tileRangeKeyForBounds(bounds, viewportZoom, HIGH_RES_PREFETCH_VIEWPORT_BUFFER_TILES);
    const userTile = hasUser ? latLonToTileXY(userLat, userLon, maxNativeZoom) : null;
    const rocketTile = hasRocket ? latLonToTileXY(rocketLat, rocketLon, maxNativeZoom) : null;
    const key = [tilesUrl, String(maxNativeZoom || ""), Number.isFinite(mapZoom) ? Math.floor(mapZoom).toString() : "", userTile ? `${userTile.x}:${userTile.y}` : "", rocketTile ? `${rocketTile.x}:${rocketTile.y}` : "", userRadiusM.toFixed(0), rocketRadiusM.toFixed(0), viewportKey,].join("|");

    if (nativeDesktop) {
        const segments = [];
        let userTiles = 0;
        let rocketTiles = 0;
        let combinedTiles = 0;
        const combinedSeen = new Set();
        const countCombinedRange = (range) => {
            if (!range) return 0;
            let added = 0;
            for (let x = range.xMin; x <= range.xMax; x++) {
                for (let y = range.yMin; y <= range.yMax; y++) {
                    const id = `${range.z}/${x}/${y}`;
                    if (combinedSeen.has(id)) continue;
                    combinedSeen.add(id);
                    added += 1;
                }
            }
            return added;
        };
        for (const zoom of zooms) {
            if (bounds) {
                const range = tileRangeForBounds(bounds, zoom, HIGH_RES_PREFETCH_VIEWPORT_BUFFER_TILES);
                if (range) {
                    segments.push({kind: "viewport", range});
                    combinedTiles += countCombinedRange(range);
                }
            }
            if (hasUser) {
                const range = tileRangeForRadius(userLat, userLon, zoom, userRadiusM);
                if (range) {
                    segments.push({kind: "user", range});
                    userTiles += tileCountForRange(range);
                    combinedTiles += countCombinedRange(range);
                }
            }
            if (hasRocket) {
                const range = tileRangeForRadius(rocketLat, rocketLon, zoom, rocketRadiusM);
                if (range) {
                    segments.push({kind: "rocket", range});
                    rocketTiles += tileCountForRange(range);
                    combinedTiles += countCombinedRange(range);
                }
            }
        }
        return {
            key,
            coords: [],
            totalTiles: combinedTiles,
            segments,
            breakdown: {userTiles, rocketTiles, combinedTiles},
        };
    }

    for (const zoom of zooms) {
        if (bounds) {
            appendUniqueCoords(coords, seen, tileCoordsForBounds(bounds, zoom));
        }

        if (hasUser) {
            const aroundUser = tileCoordsAround(userLat, userLon, zoom, userRadiusM);
            appendUniqueCoords(userCoords, userSeen, aroundUser);
            appendUniqueCoords(coords, seen, aroundUser);
        }

        if (hasRocket) {
            const aroundRocket = tileCoordsAround(rocketLat, rocketLon, zoom, rocketRadiusM);
            appendUniqueCoords(rocketCoords, rocketSeen, aroundRocket);
            appendUniqueCoords(coords, seen, aroundRocket);
        }
    }

    return {
        key, coords, breakdown: {
            userTiles: userCoords.length, rocketTiles: rocketCoords.length, combinedTiles: coords.length,
        },
    };
}

async function runHighResTilePrefetch(runId, key) {
    const tilesUrl = effectivePrefetchTilesUrl();
    if (!tilesUrl) return;

    const plan = buildHighResPrefetchPlan();
    const estimate = setTilePrefetchEstimate(plan);
    if (estimate.tooLarge) {
        setTilePrefetchState({
            key,
            state: "budget-low",
            pending: 0,
            completed: 0,
            failed: 0,
            estimatedBytes: estimate.estimatedBytes,
            budgetBytes: estimate.budgetBytes,
        });
        return;
    }
    if (!plan.coords.length) {
        setTilePrefetchState({
            key, state: "ready", pending: 0, completed: 0, failed: 0, lastCompletedAt: Date.now(),
        });
        return;
    }

    const cacheName = tileCacheName(tilesUrl);
    let nextIndex = 0;
    let completed = 0;
    let failed = 0;
    const total = plan.coords.length;
    let lastStateUpdateAt = 0;

    setTilePrefetchState({
        key, state: "warming", pending: total, completed: 0, failed: 0, lastStartedAt: Date.now(),
    });

    const publishProgress = (force = false) => {
        if (runId !== tilePrefetchRunId) return;
        const now = Date.now();
        if (!force && completed < total && now - lastStateUpdateAt < HIGH_RES_PREFETCH_STATE_UPDATE_INTERVAL_MS) {
            return;
        }
        lastStateUpdateAt = now;
        setTilePrefetchState({
            key,
            state: completed >= total ? "ready" : "warming",
            pending: Math.max(0, total - completed),
            completed,
            failed,
            lastCompletedAt: completed >= total ? now : tilePrefetchState.lastCompletedAt,
        });
    };

    const processNativeDesktopSegments = async () => {
        const seen = new Set();
        for (const segment of Array.isArray(plan.segments) ? plan.segments : []) {
            const range = segment && segment.range;
            if (!range) continue;
            for (let x = range.xMin; x <= range.xMax; x++) {
                for (let y = range.yMin; y <= range.yMax; y++) {
                    if (runId !== tilePrefetchRunId || effectivePrefetchTilesUrl() !== tilesUrl) return;
                    const id = `${range.z}/${x}/${y}`;
                    if (seen.has(id)) continue;
                    seen.add(id);
                    const url = resolvePrefetchTileUrl(tilesUrl, range.z, x, y);
                    if (!url || isKnownMissingTile(cacheName, url)) {
                        completed += 1;
                        publishProgress();
                        continue;
                    }
                    try {
                        await prefetchTileToPersistentCache(cacheName, url);
                    } catch (e) {
                        failed += 1;
                    } finally {
                        completed += 1;
                        publishProgress();
                        await yieldBrowserTileWork();
                    }
                }
            }
        }
    };

    if (isDesktopNativeMapRuntime() && Array.isArray(plan.segments)) {
        await processNativeDesktopSegments();
        if (runId === tilePrefetchRunId) {
            publishProgress(true);
        }
        return;
    }

    const worker = async () => {
        while (true) {
            if (runId !== tilePrefetchRunId || effectivePrefetchTilesUrl() !== tilesUrl) return;
            const index = nextIndex++;
            if (index >= total) return;
            const coord = plan.coords[index];
            const url = resolvePrefetchTileUrl(tilesUrl, coord.z, coord.x, coord.y);
            if (!url || isKnownMissingTile(cacheName, url)) {
                completed += 1;
                publishProgress();
                continue;
            }
            try {
                await prefetchTileToPersistentCache(cacheName, url);
            } catch (e) {
                failed += 1;
            } finally {
                completed += 1;
                publishProgress();
                await yieldBrowserTileWork();
            }
        }
    };

    const concurrency = Math.max(1, Math.min(highResPrefetchConcurrencyLimit(), total));
    await Promise.allSettled(Array.from({length: concurrency}, () => worker()));

    if (runId === tilePrefetchRunId) {
        publishProgress(true);
    }
}

async function runTrackingTilePrefetch(runId, plan) {
    if (tileTrackingPrefetchActive) return;
    const tilesUrl = effectivePrefetchTilesUrl();
    if (!tilesUrl || !plan || !plan.coords.length) return;

    tileTrackingPrefetchActive = true;
    const cacheName = tileCacheName(tilesUrl);
    let nextIndex = 0;
    const total = plan.coords.length;

    const worker = async () => {
        while (true) {
            if (runId !== tileTrackingPrefetchRunId || effectivePrefetchTilesUrl() !== tilesUrl) return;
            const index = nextIndex++;
            if (index >= total) return;
            const coord = plan.coords[index];
            const url = resolvePrefetchTileUrl(tilesUrl, coord.z, coord.x, coord.y);
            if (!url || isKnownMissingTile(cacheName, url)) continue;
            try {
                await prefetchTileToPersistentCache(cacheName, url);
            } catch (e) {
            }
            await yieldBrowserTileWork();
        }
    };

    try {
        const concurrency = Math.max(1, Math.min(trackingPrefetchConcurrencyLimit(), total));
        await Promise.allSettled(Array.from({length: concurrency}, () => worker()));
    } finally {
        tileTrackingPrefetchActive = false;
    }
}

function ensureTrackingTilePrefetchLoop() {
    if (tileTrackingPrefetchInterval) return;
    tileTrackingPrefetchInterval = setInterval(safeMapCallback("tracking tile prefetch interval", () => {
        scheduleTrackingTilePrefetch({force: true});
    }), trackingPrefetchIntervalMs());
}

function scheduleTrackingTilePrefetch(options = {}) {
    if (!shouldRunBrowserMapPrefetch()) {
        stopTrackingTilePrefetch();
        refreshTilePrefetchEstimate();
        return;
    }
    const force = options.force === true;
    const tilesUrl = effectivePrefetchTilesUrl();
    if (!tilesUrl || !mapPrefetchEnabled() || !tileFetchAllowedForUrl(tilesUrl) || !tilePrefetchSupported()) {
        stopTrackingTilePrefetch();
        refreshTilePrefetchEstimate();
        return;
    }
    const plan = buildTrackingPrefetchPlan();
    if (!plan.key || !plan.coords.length) {
        stopTrackingTilePrefetch();
        refreshTilePrefetchEstimate();
        return;
    }

    ensureTrackingTilePrefetchLoop();
    if (!force && currentTrackingPrefetchKey === plan.key) {
        return;
    }
    currentTrackingPrefetchKey = plan.key;

    if (tileTrackingPrefetchTimer) {
        cancelIdleDelay(tileTrackingPrefetchTimer);
    }
    const runId = ++tileTrackingPrefetchRunId;
    tileTrackingPrefetchTimer = idleDelay(safeMapCallback("tracking tile prefetch timer", async () => {
        tileTrackingPrefetchTimer = null;
        await runTrackingTilePrefetch(runId, plan);
    }), trackingPrefetchDelayMs());
}

function scheduleTileCacheSweep(tilesUrl) {
    if (!tileCacheEnabled() || !tileCacheSupported()) return;
    if (tileCacheSweepTimer) clearTimeout(tileCacheSweepTimer);
    tileCacheSweepTimer = setTimeout(safeMapCallback("tile cache sweep timer", async () => {
        try {
            const active = tileCacheName(tilesUrl);
            const keys = await caches.keys();
            await Promise.all(keys
                .filter((key) => isGroundMapTileCacheName(key) && key !== active)
                .map((key) => caches.delete(key)));
        } catch (e) {
            if (mapDebugLoggingEnabled()) console.warn("[GS26 map] cache sweep failed", e);
        }
    }), CACHE_SWEEP_DELAY_MS);
}

function scheduleHighResTilePrefetch(options = {}) {
    const force = options && options.force === true;
    if (!force && !shouldRunAutomaticHighResPrefetch()) {
        refreshTilePrefetchEstimate();
        return;
    }
    if (!shouldRunBrowserMapPrefetch()) {
        currentPrefetchKey = "";
        if (tilePrefetchTimer) cancelIdleDelay(tilePrefetchTimer);
        tilePrefetchTimer = null;
        stopTrackingTilePrefetch();
        refreshTilePrefetchEstimate();
        setTilePrefetchState({
            key: "",
            state: (isBrowserHostedMapRuntime() || isIosPlatform() || isAndroidPlatform()) && isGroundMapVisibleForPrefetch() ? "suspended-visible-map" : "idle",
            detail: (isBrowserHostedMapRuntime() || isIosPlatform() || isAndroidPlatform()) && isGroundMapVisibleForPrefetch() ? MAP_VISIBLE_PREFETCH_SUSPEND_DETAIL : "",
            pending: 0,
            completed: 0,
            failed: 0,
        });
        return;
    }
    const tilesUrl = effectivePrefetchTilesUrl();
    if (!tilesUrl) return;
    if (!force && !mapPrefetchEnabled()) {
        currentPrefetchKey = "";
        if (tilePrefetchTimer) cancelIdleDelay(tilePrefetchTimer);
        tilePrefetchTimer = null;
        stopTrackingTilePrefetch();
        refreshTilePrefetchEstimate();
        setTilePrefetchState({
            key: "", state: "disabled", detail: "Map prefetch is disabled.", pending: 0, completed: 0, failed: 0,
        });
        return;
    }
    if (!tileCacheEnabled() || !tileFetchAllowedForUrl(tilesUrl) || !tilePrefetchSupported()) {
        currentPrefetchKey = "";
        stopTrackingTilePrefetch();
        refreshTilePrefetchEstimate();
        setTilePrefetchState({
            key: "", state: "idle", pending: 0, completed: 0, failed: 0,
        });
        return;
    }
    const plan = buildHighResPrefetchPlan();
    const estimate = setTilePrefetchEstimate(plan);
    if (estimate.tooLarge) {
        currentPrefetchKey = "";
        if (tilePrefetchTimer) cancelIdleDelay(tilePrefetchTimer);
        tilePrefetchTimer = null;
        setTilePrefetchState({
            key: plan.key || "",
            state: "budget-low",
            pending: 0,
            completed: 0,
            failed: 0,
            estimatedBytes: estimate.estimatedBytes,
            budgetBytes: estimate.budgetBytes,
        });
        return;
    }
    const key = plan.key;
    if (!key) {
        const context = publishTilePrefetchContextState();
        refreshTilePrefetchEstimate();
        currentPrefetchKey = "";
        if (tilePrefetchTimer) cancelIdleDelay(tilePrefetchTimer);
        tilePrefetchTimer = null;
        setTilePrefetchState({
            key: "",
            state: context.userAvailable || context.rocketAvailable ? "idle" : "waiting-context",
            detail: context.summaryMessage,
            pending: 0,
            completed: 0,
            failed: 0,
        });
        return;
    }
    if (!force && currentPrefetchKey === key) return;
    currentPrefetchKey = key;

    if (tilePrefetchTimer) cancelIdleDelay(tilePrefetchTimer);
    const runId = ++tilePrefetchRunId;
    setTilePrefetchState({
        key, state: "queued", pending: plan.coords.length, completed: 0, failed: 0,
    });
    const now = Date.now();
    const sinceInitMs = mapInitStartedAtMs > 0 ? Math.max(0, now - mapInitStartedAtMs) : HIGH_RES_PREFETCH_STARTUP_DELAY_MS;
    const startupDelayMs = Math.max(highResPrefetchIdleDelayMs(), HIGH_RES_PREFETCH_STARTUP_DELAY_MS - sinceInitMs);
    const suppressionDelayMs = Math.max(0, prefetchSuppressedUntilMs - now);
    const delayMs = force ? 0 : Math.max(startupDelayMs, suppressionDelayMs);
    tilePrefetchTimer = idleDelay(safeMapCallback("high-res tile prefetch timer", async () => {
        tilePrefetchTimer = null;
        if (!force && Date.now() < prefetchSuppressedUntilMs) {
            scheduleHighResTilePrefetch();
            return;
        }
        await runHighResTilePrefetch(runId, key);
        if (runId === tilePrefetchRunId) {
            autoHighResPrefetchCompleted = true;
        }
    }), delayMs);
}

function prefetchGroundMapTilesNow() {
    scheduleHighResTilePrefetch({force: true});
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
      display: flex !important;
      align-items: center;
      justify-content: center;
      padding: 0 !important;
      line-height: 0;
    }

    .gs26-map-north-control[hidden] {
      display: none !important;
    }

    .gs26-map-north-control-icon {
      position: relative;
      width: 18px;
      height: 18px;
      display: block;
      flex: 0 0 auto;
      margin: 0;
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
        x: (touches[0].clientX + touches[1].clientX) / 2, y: (touches[0].clientY + touches[1].clientY) / 2,
    };
}

function updateUserMarkerRotation() {
    syncUserHeadingIndicator();
}

function updateUserMarkerRotationThrottled(force = false) {
    const now = typeof performance !== "undefined" && typeof performance.now === "function" ? performance.now() : Date.now();
    if (!force && now - lastHeadingVisualSyncAtMs < USER_HEADING_VISUAL_SYNC_MIN_MS) return;
    lastHeadingVisualSyncAtMs = now;
    updateUserMarkerRotation();
}

function centerControlMode() {
    if (followUserEnabled && orientationMode === "user" && hasUsableUserHeading()) return "user-up";
    if (followUserEnabled) return "follow";
    return "idle";
}

function updateCenterControlAppearance() {
    if (!mapCenterControl || !mapCenterControl._button) return;
    const mode = centerControlMode();
    mapCenterControl._button.setAttribute("data-mode", mode);
    mapCenterControl._button.title = mode === "user-up" ? "User Up Enabled" : mode === "follow" ? "Auto Center Enabled" : "Center On Me";
    mapCenterControl._button.setAttribute("aria-label", mode === "user-up" ? "User Up Enabled" : mode === "follow" ? "Auto Center Enabled" : "Center On Me");
    updateNorthControlAppearance();
    updateZoomControlAppearance();
}

function findMapControlButton(className) {
    try {
        const container = groundMap && groundMap.getContainer ? groundMap.getContainer() : document;
        return container ? container.querySelector(`.${className}`) : null;
    } catch (e) {
        return null;
    }
}

function setZoomButtonState(button, enabled, label) {
    if (!button) return;
    button.disabled = !enabled;
    button.setAttribute("aria-disabled", enabled ? "false" : "true");
    button.title = enabled ? label : `${label} unavailable`;
}

function updateZoomControlAppearance() {
    if (!groundMap) return;
    const zoom = Number(groundMap.getZoom && groundMap.getZoom());
    const minZoom = effectiveMinZoom();
    const maxZoom = Number.isFinite(currentMaxZoom) ? currentMaxZoom : DEFAULT_MAX_NATIVE_ZOOM + DEFAULT_MAX_OVERZOOM_DELTA;
    const canZoomIn = Number.isFinite(zoom) && zoom < maxZoom - WHEEL_ZOOM_LIMIT_EPSILON;
    const canZoomOut = Number.isFinite(zoom) && zoom > minZoom + WHEEL_ZOOM_LIMIT_EPSILON;
    setZoomButtonState(findMapControlButton("maplibregl-ctrl-zoom-in"), canZoomIn, "Zoom In");
    setZoomButtonState(findMapControlButton("maplibregl-ctrl-zoom-out"), canZoomOut, "Zoom Out");
}

function applyZoomButtonCameraFrame(zoom) {
    const nextZoom = Number(zoom);
    if (!groundMap || !Number.isFinite(nextZoom)) return false;
    const followCenter = currentFollowUserZoomCenter();
    markInternalCameraUpdate(80);
    if (followCenter) {
        beginFollowZoomHold(120);
        return recenterFollowUserDuringZoom(nextZoom);
    }
    try {
        groundMap.jumpTo({
            zoom: nextZoom, bearing: mapBearingDeg,
        });
        return true;
    } catch (e) {
        return false;
    }
}

function finishZoomButtonAnimation() {
    zoomButtonAnimationFrame = null;
    zoomButtonAnimationLastAtMs = 0;
    zoomButtonTargetZoom = null;
    rememberMapView();
    persistMapStateSoon();
    scheduleTrackingTilePrefetch();
    updateZoomControlAppearance();
}

function cancelZoomButtonAnimation() {
    if (zoomButtonAnimationFrame != null) {
        try {
            cancelAnimationFrame(zoomButtonAnimationFrame);
        } catch (e) {
        }
        zoomButtonAnimationFrame = null;
    }
    zoomButtonAnimationLastAtMs = 0;
}

function startOrRetargetZoomButtonAnimation(targetZoom) {
    if (!groundMap || !Number.isFinite(Number(targetZoom))) return false;
    zoomButtonTargetZoom = Number(targetZoom);
    if (zoomButtonAnimationFrame != null) {
        return true;
    }
    zoomButtonAnimationLastAtMs = performance.now();
    const step = safeMapCallback("zoom button animation frame", () => {
        if (!groundMap || !Number.isFinite(zoomButtonTargetZoom)) {
            finishZoomButtonAnimation();
            return;
        }
        const now = performance.now();
        const elapsedSeconds = Math.max(0.001, Math.min(0.05, (now - zoomButtonAnimationLastAtMs) / 1000));
        zoomButtonAnimationLastAtMs = now;

        const currentZoom = Number(groundMap.getZoom && groundMap.getZoom());
        if (!Number.isFinite(currentZoom)) {
            finishZoomButtonAnimation();
            return;
        }
        const diff = zoomButtonTargetZoom - currentZoom;
        if (Math.abs(diff) <= ZOOM_BUTTON_SETTLE_EPSILON) {
            applyZoomButtonCameraFrame(zoomButtonTargetZoom);
            finishZoomButtonAnimation();
            return;
        }

        const distanceSpeedup = Math.min(ZOOM_BUTTON_MAX_DISTANCE_SPEEDUP, Math.max(1, Math.abs(diff)));
        const maxStep = ZOOM_BUTTON_UNITS_PER_SECOND * distanceSpeedup * elapsedSeconds;
        const nextZoom = currentZoom + Math.sign(diff) * Math.min(Math.abs(diff), maxStep);
        if (!applyZoomButtonCameraFrame(nextZoom)) {
            finishZoomButtonAnimation();
            return;
        }
        updateZoomControlAppearance();
        zoomButtonAnimationFrame = requestAnimationFrame(step);
    });
    zoomButtonAnimationFrame = requestAnimationFrame(step);
    return true;
}

function applyImmediateZoomStep(delta) {
    if (!groundMap) return;
    const currentZoom = Number(groundMap.getZoom && groundMap.getZoom());
    if (!Number.isFinite(currentZoom)) return;
    const maxZoom = Number.isFinite(currentMaxZoom) ? currentMaxZoom : DEFAULT_MAX_NATIVE_ZOOM + DEFAULT_MAX_OVERZOOM_DELTA;
    const baseZoom = Number.isFinite(zoomButtonTargetZoom) ? zoomButtonTargetZoom : currentZoom;
    const nextZoomTarget = delta > 0 ? Math.floor(baseZoom + WHEEL_ZOOM_LIMIT_EPSILON) + 1 : Math.ceil(baseZoom - WHEEL_ZOOM_LIMIT_EPSILON) - 1;
    const nextZoom = Math.max(effectiveMinZoom(), Math.min(maxZoom, nextZoomTarget));
    if (Math.abs(nextZoom - currentZoom) <= WHEEL_ZOOM_LIMIT_EPSILON && Math.abs(nextZoom - baseZoom) <= WHEEL_ZOOM_LIMIT_EPSILON) {
        updateZoomControlAppearance();
        return;
    }
    zoomButtonTargetZoom = nextZoom;
    try {
        if (typeof groundMap.stop === "function") {
            groundMap.stop();
        }
    } catch (e) {
    }
    startOrRetargetZoomButtonAnimation(nextZoom);
    scheduleTrackingTilePrefetch();
    updateZoomControlAppearance();
}

function installImmediateZoomButtonHandlers() {
    if (!groundMap) return;
    const zoomIn = findMapControlButton("maplibregl-ctrl-zoom-in");
    const zoomOut = findMapControlButton("maplibregl-ctrl-zoom-out");
    const install = (button, delta) => {
        if (!button || button.__gs26_immediate_zoom_installed) return;
        button.__gs26_immediate_zoom_installed = true;
        button.addEventListener("click", safeMapCallback("zoom control click", (event) => {
            event.preventDefault();
            event.stopPropagation();
            if (typeof event.stopImmediatePropagation === "function") {
                event.stopImmediatePropagation();
            }
            if (button.disabled || button.getAttribute("aria-disabled") === "true") return;
            applyImmediateZoomStep(delta);
        }), {capture: true});
    };
    install(zoomIn, 1);
    install(zoomOut, -1);
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
    const dataKey = guideLineDataKey(rocketLatLng, userLatLng);
    if (source.__gs26_data_key === dataKey) return;

    if (!rocketLatLng || !userLatLng) {
        source.__gs26_data_key = SOURCE_EMPTY_KEY;
        source.setData({
            type: "FeatureCollection", features: [],
        });
        return;
    }

    source.__gs26_data_key = dataKey;
    source.setData({
        type: "FeatureCollection", features: [{
            type: "Feature", geometry: {
                type: "LineString", coordinates: [[userLatLng[1], userLatLng[0]], [rocketLatLng[1], rocketLatLng[0]],],
            }, properties: {},
        }],
    });
}

function syncPointSource(sourceId, latLng) {
    if (!groundMap || !mapReady) return;
    const source = groundMap.getSource(sourceId);
    if (!source) return;
    const dataKey = pointDataKey(latLng);
    if (source.__gs26_data_key === dataKey) return;
    source.__gs26_data_key = dataKey;
    source.setData(pointFeatureCollection(latLng));
}

function syncUserHeadingIndicator() {
    if (!groundMap || !mapReady) return;
    const source = groundMap.getSource(USER_HEADING_SOURCE_ID);
    if (!source) return;
    if (!hasUsableUserHeading()) {
        if (source.__gs26_data_key === SOURCE_EMPTY_KEY) return;
        source.__gs26_data_key = SOURCE_EMPTY_KEY;
        source.setData(emptyFeatureCollection());
        return;
    }
    const latLng = currentUserVisualOrLastLatLng();
    const displayHeadingDeg = Number.isFinite(userHeadingArrowDeg) ? userHeadingArrowDeg : (Number.isFinite(userHeadingIndicatorDeg) ? userHeadingIndicatorDeg : (Number.isFinite(userHeadingDisplayDeg) ? userHeadingDisplayDeg : userHeadingDeg));
    const dataKey = headingDataKey(latLng, displayHeadingDeg);
    if (source.__gs26_data_key === dataKey) return;
    source.__gs26_data_key = dataKey;
    source.setData(headingFeatureCollection(latLng, displayHeadingDeg));
}

function snapFollowCameraTo(latLng) {
    if (!groundMap || !Array.isArray(latLng)) return;
    pendingFollowCameraLatLng = null;
    followCameraVisualPoint = null;
    followCameraVelocityXPps = 0;
    followCameraVelocityYPps = 0;
    if (followCameraFrame != null) {
        try {
            cancelAnimationFrame(followCameraFrame);
        } catch (e) {
        }
        followCameraFrame = null;
    }
    if (!followUserEnabled || Date.now() < suppressFollowCameraUntilMs) return;
    followCameraCenterLocked = true;
    markInternalCameraUpdate(120);
    try {
        groundMap.jumpTo({
            center: [latLng[1], latLng[0]], bearing: mapBearingDeg,
        });
        rememberMapView();
        persistMapStateSoon();
        syncWindowMapControlState();
    } catch (e) {
    }
}

function currentFollowUserZoomCenter() {
    if (!followUserEnabled) return null;
    const visual = currentUserVisualOrLastLatLng();
    if (!Array.isArray(visual)) return null;
    const lat = Number(visual[0]);
    const lon = Number(visual[1]);
    if (!Number.isFinite(lat) || !Number.isFinite(lon)) return null;
    return [lon, lat];
}

function cancelFollowCameraAnimation() {
    pendingFollowCameraLatLng = null;
    followCameraVisualPoint = null;
    followCameraVelocityXPps = 0;
    followCameraVelocityYPps = 0;
    followCameraCenterLocked = false;
    if (followCameraFrame != null) {
        try {
            cancelAnimationFrame(followCameraFrame);
        } catch (e) {
        }
        followCameraFrame = null;
    }
}

function cancelFollowZoomAnimation() {
    if (followZoomAnimationFrame != null) {
        try {
            cancelAnimationFrame(followZoomAnimationFrame);
        } catch (e) {
        }
        followZoomAnimationFrame = null;
    }
}

function beginFollowZoomHold(durationMs) {
    const until = Date.now() + Math.max(120, Number(durationMs) || 0) + 140;
    followZoomHoldUntilMs = Math.max(followZoomHoldUntilMs, until);
    suppressFollowCameraUntilMs = Math.max(suppressFollowCameraUntilMs, until);
    cancelFollowCameraAnimation();
}

function isFollowZoomHolding() {
    return followUserEnabled && Date.now() < followZoomHoldUntilMs;
}

function recenterFollowUserDuringZoom(zoom) {
    const center = currentFollowUserZoomCenter();
    if (!groundMap || !center) return false;
    const nextZoom = Number(zoom);
    markInternalCameraUpdate(80);
    try {
        const options = {
            center, bearing: mapBearingDeg,
        };
        if (Number.isFinite(nextZoom)) {
            options.zoom = nextZoom;
        }
        groundMap.jumpTo(options);
        return true;
    } catch (e) {
        return false;
    }
}

function animateFollowZoomTo(targetZoom, durationMs) {
    if (!groundMap) return false;
    const startZoom = Number(groundMap.getZoom && groundMap.getZoom());
    const endZoom = Number(targetZoom);
    if (!Number.isFinite(startZoom) || !Number.isFinite(endZoom)) return false;
    cancelFollowZoomAnimation();
    beginFollowZoomHold(durationMs);
    const startedAt = performance.now();
    const duration = Math.max(1, Number(durationMs) || ZOOM_BUTTON_ANIMATION_MS);
    const ease = (t) => t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
    const step = safeMapCallback("follow zoom animation frame", () => {
        if (!groundMap || !followUserEnabled) {
            followZoomAnimationFrame = null;
            return;
        }
        const elapsed = performance.now() - startedAt;
        const t = Math.max(0, Math.min(1, elapsed / duration));
        const zoom = startZoom + (endZoom - startZoom) * ease(t);
        if (!recenterFollowUserDuringZoom(zoom)) {
            followZoomAnimationFrame = null;
            return;
        }
        updateZoomControlAppearance();
        if (t >= 1) {
            followZoomAnimationFrame = null;
            followZoomHoldUntilMs = Math.max(followZoomHoldUntilMs, Date.now() + 180);
            return;
        }
        followZoomAnimationFrame = requestAnimationFrame(step);
    });
    followZoomAnimationFrame = requestAnimationFrame(step);
    return true;
}

function holdFollowUserAtScreenCenterForZoom(options = {}) {
    if (!groundMap) return null;
    const center = currentFollowUserZoomCenter();
    if (!center) return null;
    beginFollowZoomHold(options && Number.isFinite(Number(options.durationMs)) ? Number(options.durationMs) : 120);
    const zoom = Number(options.zoom);
    markInternalCameraUpdate(120);
    try {
        const jumpOptions = {
            center, bearing: mapBearingDeg,
        };
        if (Number.isFinite(zoom)) {
            jumpOptions.zoom = zoom;
        }
        groundMap.jumpTo(jumpOptions);
    } catch (e) {
    }
    return center;
}

function applyFollowWheelZoom(deltaY) {
    if (!groundMap || !followUserEnabled) return false;
    const center = currentFollowUserZoomCenter();
    if (!center) return false;
    const currentZoom = Number(groundMap.getZoom && groundMap.getZoom());
    if (!Number.isFinite(currentZoom)) return false;
    const dy = Number(deltaY);
    if (!Number.isFinite(dy) || Math.abs(dy) < 0.01) return false;
    const maxZoom = Number.isFinite(currentMaxZoom) ? currentMaxZoom : DEFAULT_MAX_NATIVE_ZOOM + DEFAULT_MAX_OVERZOOM_DELTA;
    const nextZoom = Math.max(effectiveMinZoom(), Math.min(maxZoom, currentZoom - dy * 0.004));
    if (Math.abs(nextZoom - currentZoom) <= WHEEL_ZOOM_LIMIT_EPSILON) {
        holdFollowUserAtScreenCenterForZoom({zoom: currentZoom});
        updateZoomControlAppearance();
        return true;
    }
    zoomButtonTargetZoom = null;
    suppressHighResPrefetch(300);
    holdFollowUserAtScreenCenterForZoom({zoom: nextZoom});
    rememberMapView();
    persistMapStateSoon();
    scheduleTrackingTilePrefetch();
    updateZoomControlAppearance();
    return true;
}

function followCameraDistancePx(latLng) {
    if (!groundMap || !Array.isArray(latLng)) return Infinity;
    try {
        const center = groundMap.getCenter();
        const centerPoint = groundMap.project(center);
        const targetPoint = groundMap.project({lat: latLng[0], lng: latLng[1]});
        return Math.hypot(targetPoint.x - centerPoint.x, targetPoint.y - centerPoint.y);
    } catch (e) {
        return Infinity;
    }
}

function scheduleFollowCameraUpdate(latLng) {
    if (!groundMap || !Array.isArray(latLng)) return;
    if (Date.now() < suppressFollowCameraUntilMs) return;
    if (followCameraDistancePx(latLng) <= FOLLOW_CAMERA_LOCK_DISTANCE_PX) {
        snapFollowCameraTo(latLng);
        return;
    }
    followCameraCenterLocked = false;
    pendingFollowCameraLatLng = [latLng[0], latLng[1]];
    if (followCameraFrame != null) return;
    followCameraLastFrameAt = performance.now();
    const step = safeMapCallback("follow camera frame", () => {
        const target = pendingFollowCameraLatLng;
        if (!groundMap || !followUserEnabled || !Array.isArray(target) || Date.now() < suppressFollowCameraUntilMs) {
            followCameraFrame = null;
            return;
        }
        if (isOrientationModeSettling()) {
            followCameraFrame = requestAnimationFrame(step);
            return;
        }
        const now = performance.now();
        const rawDtMs = Math.max(1.0, now - (followCameraLastFrameAt || now));
        const dtMs = Math.max(1.0, Math.min(50.0, rawDtMs));
        followCameraLastFrameAt = now;
        let center;
        let centerPoint;
        let targetPoint;
        try {
            center = groundMap.getCenter();
            centerPoint = groundMap.project(center);
            targetPoint = groundMap.project({lat: target[0], lng: target[1]});
        } catch (e) {
            followCameraFrame = null;
            return;
        }
        const errorX = targetPoint.x - centerPoint.x;
        const errorY = targetPoint.y - centerPoint.y;
        const distancePx = Math.hypot(errorX, errorY);
        if (rawDtMs >= FOLLOW_CAMERA_FRAME_STALL_SNAP_MS) {
            snapFollowCameraTo(target);
            followCameraVisualPoint = targetPoint;
            followCameraVelocityXPps = 0;
            followCameraVelocityYPps = 0;
            pendingFollowCameraLatLng = null;
            followCameraFrame = null;
            return;
        }
        if (distancePx <= FOLLOW_CAMERA_LOCK_DISTANCE_PX) {
            snapFollowCameraTo(target);
            followCameraVisualPoint = targetPoint;
            followCameraVelocityXPps = 0;
            followCameraVelocityYPps = 0;
            pendingFollowCameraLatLng = null;
            followCameraFrame = null;
            return;
        }
        const alpha = Math.min(1.0, 1.0 - Math.exp(-dtMs / FOLLOW_CAMERA_CATCHUP_MS));
        let stepX = errorX * alpha;
        let stepY = errorY * alpha;
        let stepLen = Math.hypot(stepX, stepY);
        if (stepLen >= distancePx - FOLLOW_CAMERA_SETTLE_EPSILON_PX) {
            stepX = errorX;
            stepY = errorY;
            stepLen = distancePx;
        }
        const adaptiveMaxStepPx = Math.max(FOLLOW_CAMERA_MIN_STEP_PX, Math.min(FOLLOW_CAMERA_MAX_STEP_PX, distancePx * FOLLOW_CAMERA_STEP_DISTANCE_RATIO));
        if (stepLen > adaptiveMaxStepPx) {
            const scale = adaptiveMaxStepPx / stepLen;
            stepX *= scale;
            stepY *= scale;
        }
        followCameraVelocityXPps = 0;
        followCameraVelocityYPps = 0;
        followCameraVisualPoint = {
            x: centerPoint.x + stepX, y: centerPoint.y + stepY,
        };
        let nextCenter;
        try {
            nextCenter = groundMap.unproject(followCameraVisualPoint);
        } catch (e) {
            followCameraFrame = null;
            return;
        }
        markInternalCameraUpdate(80);
        try {
            groundMap.jumpTo({
                center: [nextCenter.lng, nextCenter.lat], bearing: mapBearingDeg,
            });
        } catch (e) {
            followCameraFrame = null;
            return;
        }
        syncWindowMapControlState();
        persistMapStateSoon();
        followCameraFrame = requestAnimationFrame(step);
    });
    followCameraFrame = requestAnimationFrame(step);
}

function setUserMarkerVisualLatLng(latLng, options = {}) {
    if (!Array.isArray(latLng)) return;
    userMarkerDisplayedLatLng = [latLng[0], latLng[1]];
    const followTarget = options && Array.isArray(options.followTarget) ? options.followTarget : userMarkerDisplayedLatLng;
    if (followUserEnabled && groundMap && options.skipFollow !== true) {
        if (isFollowZoomHolding()) {
            recenterFollowUserDuringZoom();
        } else if (followCameraCenterLocked) {
            snapFollowCameraTo(followTarget);
        } else if (followCameraDistancePx(followTarget) <= FOLLOW_CAMERA_LOCK_DISTANCE_PX) {
            snapFollowCameraTo(followTarget);
        } else {
            scheduleFollowCameraUpdate(followTarget);
        }
    }
    syncPointSource(USER_SOURCE_ID, userMarkerDisplayedLatLng);
    syncUserHeadingIndicator();
    syncRocketGuideLine(lastRocketLatLng, currentUserVisualOrLastLatLng());
}

function currentUserMarkerVisualLatLng() {
    const displayed = Array.isArray(userMarkerDisplayedLatLng) ? userMarkerDisplayedLatLng : null;
    const anim = userMarkerAnimation;
    if (!anim) return displayed;
    return displayed || (Array.isArray(anim.target) ? [anim.target[0], anim.target[1]] : null);
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
    followCameraLastFrameAt = 0;
    followCameraVisualPoint = null;
    followCameraVelocityXPps = 0;
    followCameraVelocityYPps = 0;
    if (userMarkerAnimationFrame != null) {
        try {
            cancelAnimationFrame(userMarkerAnimationFrame);
        } catch (e) {
        }
    }
    userMarkerAnimationFrame = null;
    userMarkerAnimation = null;
}

function resetUserMotionSmoothing(latLng) {
    if (!Array.isArray(latLng)) return;
    cancelUserMarkerAnimation();
    userMarkerDisplayedLatLng = [latLng[0], latLng[1]];
    if (followUserEnabled && groundMap) {
        snapFollowCameraTo(userMarkerDisplayedLatLng);
    }
    syncPointSource(USER_SOURCE_ID, userMarkerDisplayedLatLng);
    syncUserHeadingIndicator();
    syncRocketGuideLine(lastRocketLatLng, currentUserVisualOrLastLatLng());
}

function shouldSnapUserGpsJump(from, target, previousTarget, previousFixAtMs) {
    if (!Array.isArray(from) || !Array.isArray(target)) return false;
    const visualDistanceM = distanceMetersBetween(from, target);
    if (!Number.isFinite(visualDistanceM) || visualDistanceM < USER_GPS_SNAP_DISTANCE_M) {
        return false;
    }
    if (!Array.isArray(previousTarget) || !Number.isFinite(previousFixAtMs)) {
        return true;
    }
    const dtSec = Math.max(0.001, (Date.now() - previousFixAtMs) / 1000.0);
    const fixDistanceM = distanceMetersBetween(previousTarget, target);
    const speedMps = Number.isFinite(fixDistanceM) ? fixDistanceM / dtSec : Infinity;
    return speedMps >= USER_GPS_SNAP_SPEED_MPS;
}

function animateUserMarkerTo(targetLatLng) {
    if (!Array.isArray(targetLatLng)) return;
    const target = [targetLatLng[0], targetLatLng[1]];
    if (!userMarkerHasLiveFix) {
        userMarkerHasLiveFix = true;
        resetUserMotionSmoothing(target);
        return;
    }
    const from = currentUserMarkerVisualLatLng() || target;
    const distanceM = distanceMetersBetween(from, target);
    const previousTarget = userMarkerAnimation && Array.isArray(userMarkerAnimation.target) ? userMarkerAnimation.target : null;
    const previousFixAtMs = userMarkerAnimation && Number.isFinite(userMarkerAnimation.targetFixAtMs) ? userMarkerAnimation.targetFixAtMs : NaN;

    if (shouldSnapUserGpsJump(from, target, previousTarget, previousFixAtMs)) {
        resetUserMotionSmoothing(target);
        snapFollowCameraTo(target);
        return;
    }

    if (!Number.isFinite(distanceM) || distanceM <= USER_MARKER_SMOOTH_SKIP_M) {
        cancelUserMarkerAnimation();
        setUserMarkerVisualLatLng(target);
        return;
    }

    const nowMs = Date.now();
    let velocityLatPerMs = 0.0;
    let velocityLonPerMs = 0.0;
    let fixSpeedMps = 0.0;
    let smoothedIntervalMs = userMarkerAnimation && Number.isFinite(userMarkerAnimation.smoothedIntervalMs) ? userMarkerAnimation.smoothedIntervalMs : USER_MARKER_SMOOTH_MAX_MS;
    if (Array.isArray(previousTarget) && Number.isFinite(previousFixAtMs)) {
        const dtMs = Math.max(1.0, nowMs - previousFixAtMs);
        if (dtMs <= 10_000) {
            const fixDistanceM = distanceMetersBetween(previousTarget, target);
            fixSpeedMps = Number.isFinite(fixDistanceM) ? fixDistanceM / (dtMs / 1000.0) : 0.0;
            velocityLatPerMs = (target[0] - previousTarget[0]) / dtMs;
            let lonDiff = target[1] - previousTarget[1];
            if (lonDiff > 180.0) lonDiff -= 360.0;
            if (lonDiff < -180.0) lonDiff += 360.0;
            velocityLonPerMs = lonDiff / dtMs;
            smoothedIntervalMs = Math.max(USER_MARKER_SMOOTH_MIN_MS, Math.min(USER_MARKER_SMOOTH_MAX_MS, smoothedIntervalMs * 0.7 + dtMs * 0.3));
        }
    }
    userMarkerAnimation = {
        target,
        targetFixAtMs: nowMs,
        velocityLatPerMs,
        velocityLonPerMs,
        fixSpeedMps,
        smoothedIntervalMs,
        lastFrameAt: performance.now(),
    };

    if (userMarkerAnimationFrame != null) return;

    const step = safeMapCallback("user marker animation frame", () => {
        const anim = userMarkerAnimation;
        if (!anim) {
            userMarkerAnimationFrame = null;
            return;
        }
        const now = performance.now();
        const rawDtMs = Math.max(1.0, now - (anim.lastFrameAt || now));
        const dtMs = Math.max(1.0, Math.min(80.0, rawDtMs));
        anim.lastFrameAt = now;
        const current = currentUserMarkerVisualLatLng() || anim.target;
        const fixAgeMs = Math.max(0.0, Date.now() - (anim.targetFixAtMs || Date.now()));
        const staleAtMs = Math.max(USER_MARKER_SMOOTH_MIN_MS, anim.smoothedIntervalMs * USER_MARKER_PREDICTION_STALE_RATIO);
        if (rawDtMs >= USER_MARKER_FRAME_STALL_SNAP_MS) {
            setUserMarkerVisualLatLng(anim.target, {
                followTarget: anim.target, holdCenter: followUserEnabled,
            });
            userMarkerAnimation = null;
            userMarkerAnimationFrame = null;
            return;
        }
        const predictiveLeadMs = fixAgeMs <= staleAtMs && (Number(anim.fixSpeedMps) || 0.0) >= 0.05 ? Math.max(0.0, Math.min(USER_MARKER_PREDICTION_MAX_MS, Math.max(fixAgeMs, anim.smoothedIntervalMs * USER_MARKER_PREDICTION_RATIO))) : 0.0;
        const predictedTarget = [clampLat(anim.target[0] + anim.velocityLatPerMs * predictiveLeadMs), clampLon(anim.target[1] + anim.velocityLonPerMs * predictiveLeadMs),];
        const durationMs = Math.max(USER_MARKER_RATE_MIN_CATCHUP_MS, Math.min(USER_MARKER_RATE_MAX_CATCHUP_MS, anim.smoothedIntervalMs));
        const alpha = 1.0 - Math.exp(-dtMs / durationMs);
        const remainingToPredictionM = distanceMetersBetween(current, predictedTarget);
        const dynamicSpeedMps = Math.max(USER_MARKER_VISUAL_MIN_SPEED_MPS, Math.min(USER_MARKER_VISUAL_MAX_SPEED_MPS, Math.max((Number(anim.fixSpeedMps) || 0.0) * USER_MARKER_VISUAL_SPEED_GAIN, Number.isFinite(remainingToPredictionM) ? remainingToPredictionM / (USER_MARKER_VISUAL_CATCHUP_MS / 1000.0) : 0.0)));
        const maxStepM = dynamicSpeedMps * (dtMs / 1000.0);
        const easedTarget = blendLatLngToward(current, predictedTarget, alpha);
        const next = speedLimitedLatLngToward(current, easedTarget, maxStepM);
        const remainingDistanceM = distanceMetersBetween(next, anim.target);
        if (!Array.isArray(next) || !Number.isFinite(remainingDistanceM)) {
            setUserMarkerVisualLatLng(anim.target);
            userMarkerAnimation = null;
            userMarkerAnimationFrame = null;
            return;
        }
        if (fixAgeMs > staleAtMs && remainingDistanceM <= USER_MARKER_SMOOTH_SKIP_M) {
            setUserMarkerVisualLatLng(anim.target);
            userMarkerAnimation = null;
            userMarkerAnimationFrame = null;
            return;
        }
        setUserMarkerVisualLatLng(next, {
            followTarget: next, holdCenter: followUserEnabled,
        });
        userMarkerAnimationFrame = requestAnimationFrame(step);
    });
    userMarkerAnimationFrame = requestAnimationFrame(step);
}

function fusedHeadingTarget() {
    const hasNative = Number.isFinite(nativeHeadingDeg);
    const hasDevice = Number.isFinite(deviceHeadingDeg);
    if (hasNative) return normalizeAngle(nativeHeadingDeg);
    if (hasDevice) return normalizeAngle(deviceHeadingDeg);
    return null;
}

function hasUsableUserHeading() {
    return Number.isFinite(fusedHeadingTarget()) || Number.isFinite(userHeadingDeg);
}

function currentUserUpBearingTarget() {
    if (Number.isFinite(userHeadingDisplayDeg)) return normalizeAngle(userHeadingDisplayDeg);
    if (Number.isFinite(userHeadingDeg)) return normalizeAngle(userHeadingDeg);
    const fused = fusedHeadingTarget();
    return Number.isFinite(fused) ? normalizeAngle(fused) : null;
}

function applyKnownUserUpBearingNow() {
    const target = currentUserUpBearingTarget();
    if (!Number.isFinite(target)) return false;
    if (!Number.isFinite(userHeadingDeg)) {
        userHeadingDeg = target;
    }
    if (!Number.isFinite(userHeadingDisplayDeg)) {
        userHeadingDisplayDeg = target;
    }
    if (!Number.isFinite(userHeadingCameraDeg)) {
        userHeadingCameraDeg = target;
    }
    if (!Number.isFinite(userHeadingIndicatorDeg)) {
        userHeadingIndicatorDeg = target;
    }
    mapBearingDeg = target;
    pendingUserUpRealign = false;
    return true;
}

function scheduleHeadingAnimation() {
    if (headingAnimationFrame != null) return;
    headingAnimationLastFrameAt = performance.now();
    const step = safeMapCallback("heading animation frame", () => {
        headingAnimationFrame = null;
        const now = performance.now();
        const dtMs = Math.max(1.0, Math.min(80.0, now - (headingAnimationLastFrameAt || now)));
        headingAnimationLastFrameAt = now;
        let visualChanged = false;
        let mapChanged = false;

        const arrowTarget = Number.isFinite(userHeadingDegRaw) ? userHeadingDegRaw : fusedHeadingTarget();
        if (Number.isFinite(arrowTarget)) {
            if (!Number.isFinite(userHeadingArrowDeg)) {
                userHeadingArrowDeg = normalizeAngle(arrowTarget);
                visualChanged = true;
            } else {
                const arrowDiff = shortestAngleDiff(userHeadingArrowDeg, arrowTarget);
                if (Math.abs(arrowDiff) >= USER_ORIENTATION_ARROW_DEADZONE_DEG) {
                    const alpha = 1.0 - Math.exp(-dtMs / USER_ORIENTATION_ARROW_CATCHUP_MS);
                    const arrowStep = Math.max(-USER_ORIENTATION_MAX_STEP_DEG * 2.0, Math.min(USER_ORIENTATION_MAX_STEP_DEG * 2.0, arrowDiff * alpha));
                    userHeadingArrowDeg = normalizeAngle(userHeadingArrowDeg + arrowStep);
                    visualChanged = true;
                }
            }
        }

        if (Number.isFinite(arrowTarget)) {
            if (!Number.isFinite(userHeadingDeg)) {
                userHeadingDeg = normalizeAngle(arrowTarget);
            } else {
                const filterDiff = shortestAngleDiff(userHeadingDeg, arrowTarget);
                const absFilterDiff = Math.abs(filterDiff);
                if (absFilterDiff >= USER_ORIENTATION_ARROW_DEADZONE_DEG) {
                    const gain = absFilterDiff >= USER_ORIENTATION_INPUT_DEADZONE_DEG ? (Number.isFinite(nativeHeadingDeg) ? Math.min(0.42, Math.max(0.12, absFilterDiff / 105.0)) : Math.min(0.22, Math.max(0.06, absFilterDiff / 190.0))) : USER_ORIENTATION_SMALL_ERROR_GAIN;
                    const filterStep = Math.max(-USER_ORIENTATION_MAX_STEP_DEG, Math.min(USER_ORIENTATION_MAX_STEP_DEG, filterDiff * gain));
                    userHeadingDeg = normalizeAngle(userHeadingDeg + filterStep);
                }
            }
        }

        if (Number.isFinite(userHeadingDeg)) {
            if (!Number.isFinite(userHeadingDisplayDeg)) {
                userHeadingDisplayDeg = userHeadingDeg;
            } else {
                const displayDiff = shortestAngleDiff(userHeadingDisplayDeg, userHeadingDeg);
                if (Math.abs(displayDiff) >= USER_ORIENTATION_DISPLAY_DEADZONE_DEG) {
                    const alpha = 1.0 - Math.exp(-dtMs / USER_ORIENTATION_DISPLAY_CATCHUP_MS);
                    const displayStep = Math.max(-USER_ORIENTATION_MAX_STEP_DEG, Math.min(USER_ORIENTATION_MAX_STEP_DEG, displayDiff * alpha));
                    userHeadingDisplayDeg = normalizeAngle(userHeadingDisplayDeg + displayStep);
                }
            }
        }

        const indicatorTarget = Number.isFinite(userHeadingArrowDeg) ? userHeadingArrowDeg : (Number.isFinite(userHeadingDisplayDeg) ? userHeadingDisplayDeg : userHeadingDeg);
        if (Number.isFinite(indicatorTarget)) {
            if (followUserEnabled && orientationMode === "user") {
                if (!Number.isFinite(userHeadingIndicatorDeg) || Math.abs(shortestAngleDiff(userHeadingIndicatorDeg, indicatorTarget)) >= USER_ORIENTATION_ARROW_DEADZONE_DEG) {
                    userHeadingIndicatorDeg = indicatorTarget;
                    visualChanged = true;
                }
            } else if (!Number.isFinite(userHeadingIndicatorDeg)) {
                userHeadingIndicatorDeg = indicatorTarget;
                visualChanged = true;
            } else {
                const indicatorDiff = shortestAngleDiff(userHeadingIndicatorDeg, indicatorTarget);
                if (Math.abs(indicatorDiff) >= USER_ORIENTATION_INDICATOR_DEADZONE_DEG) {
                    const alpha = 1.0 - Math.exp(-dtMs / USER_ORIENTATION_INDICATOR_CATCHUP_MS);
                    const indicatorStep = Math.max(-USER_ORIENTATION_MAX_STEP_DEG, Math.min(USER_ORIENTATION_MAX_STEP_DEG, indicatorDiff * alpha));
                    userHeadingIndicatorDeg = normalizeAngle(userHeadingIndicatorDeg + indicatorStep);
                    visualChanged = true;
                }
            }
        }

        if (followUserEnabled && orientationMode === "user" && Number.isFinite(userHeadingDisplayDeg) && !isOrientationModeAnimationActive()) {
            const nextBearing = normalizeAngle(userHeadingDisplayDeg);
            if (!Number.isFinite(userHeadingCameraDeg)) {
                const currentBearing = groundMap ? normalizeAngle(groundMap.getBearing()) : (Number.isFinite(mapBearingDeg) ? normalizeAngle(mapBearingDeg) : nextBearing);
                userHeadingCameraDeg = currentBearing;
            }
            const cameraDiff = shortestAngleDiff(userHeadingCameraDeg, nextBearing);
            if (pendingUserUpRealign || Math.abs(cameraDiff) >= USER_ORIENTATION_CAMERA_SETTLE_DEG) {
                const alpha = 1.0 - Math.exp(-dtMs / USER_ORIENTATION_CAMERA_CATCHUP_MS);
                const cameraStep = Math.max(-USER_ORIENTATION_MAX_STEP_DEG, Math.min(USER_ORIENTATION_MAX_STEP_DEG, cameraDiff * alpha));
                userHeadingCameraDeg = normalizeAngle(userHeadingCameraDeg + cameraStep);
                mapBearingDeg = userHeadingCameraDeg;
                pendingUserUpRealign = false;
                mapChanged = true;
            }
        }

        if (mapChanged) {
            if (!isOrientationModeAnimationActive()) {
                applyMapOrientation();
            }
        } else if (visualChanged) {
            updateUserMarkerRotationThrottled();
        }

        const displaySettled = !Number.isFinite(userHeadingDeg) || !Number.isFinite(userHeadingDisplayDeg) || Math.abs(shortestAngleDiff(userHeadingDisplayDeg, userHeadingDeg)) < USER_ORIENTATION_DISPLAY_DEADZONE_DEG;
        const arrowSettled = !Number.isFinite(arrowTarget) || !Number.isFinite(userHeadingArrowDeg) || Math.abs(shortestAngleDiff(userHeadingArrowDeg, arrowTarget)) < USER_ORIENTATION_ARROW_DEADZONE_DEG;
        const filterSettled = !Number.isFinite(arrowTarget) || !Number.isFinite(userHeadingDeg) || Math.abs(shortestAngleDiff(userHeadingDeg, arrowTarget)) < USER_ORIENTATION_ARROW_DEADZONE_DEG;
        const indicatorSettled = !Number.isFinite(indicatorTarget) || !Number.isFinite(userHeadingIndicatorDeg) || Math.abs(shortestAngleDiff(userHeadingIndicatorDeg, indicatorTarget)) < USER_ORIENTATION_INDICATOR_DEADZONE_DEG;
        const bearingSettled = orientationMode !== "user" || !followUserEnabled || !Number.isFinite(userHeadingDisplayDeg) || (Number.isFinite(userHeadingCameraDeg) && Math.abs(shortestAngleDiff(userHeadingCameraDeg, userHeadingDisplayDeg)) < USER_ORIENTATION_CAMERA_SETTLE_DEG);

        if (!(arrowSettled && filterSettled && displaySettled && indicatorSettled && bearingSettled)) {
            headingAnimationFrame = requestAnimationFrame(step);
        }
    });
    headingAnimationFrame = requestAnimationFrame(step);
}

function applyMapOrientation(options = {}) {
    if (!groundMap) return;
    const animate = options && options.animate === true;
    const durationMs = Math.max(0, Number(options && options.durationMs) || ORIENTATION_MODE_ANIMATION_MS);
    const targetBearing = normalizeAngle(mapBearingDeg);
    const currentBearing = normalizeAngle(groundMap.getBearing());
    const cameraDiff = Math.abs(shortestAngleDiff(currentBearing, targetBearing));
    const minCameraDiff = orientationMode === "user" ? 0.005 : 0.05;
    if (cameraDiff > minCameraDiff) {
        markInternalCameraUpdate(animate ? durationMs + 80 : (orientationMode === "user" ? 32 : 250));
        if (animate && typeof groundMap.easeTo === "function") {
            const targetBeforeStop = targetBearing;
            orientationModeAnimationUntilMs = Date.now() + durationMs + 40;
            orientationModeSettleUntilMs = orientationModeAnimationUntilMs + 180;
            try {
                if (typeof groundMap.stop === "function") {
                    groundMap.stop();
                }
            } catch (e) {
            }
            mapBearingDeg = targetBeforeStop;
            groundMap.easeTo({
                bearing: targetBeforeStop, duration: durationMs, easing: (t) => 1 - Math.pow(1 - t, 3), essential: true,
            });
        } else {
            groundMap.jumpTo({bearing: targetBearing});
        }
    }
    updateUserMarkerRotation();
    if (animate) {
        syncWindowMapControlState();
        persistMapStateSoon();
    } else {
        rememberMapView();
    }
}

function applyFusedHeading() {
    const target = fusedHeadingTarget();
    if (!Number.isFinite(target)) return;

    userHeadingDegRaw = target;

    if (!Number.isFinite(userHeadingDeg)) {
        userHeadingDeg = target;
    } else {
        const diff = shortestAngleDiff(userHeadingDeg, target);
        const absDiff = Math.abs(diff);
        if (absDiff >= USER_ORIENTATION_ARROW_DEADZONE_DEG) {
            const gain = absDiff >= USER_ORIENTATION_INPUT_DEADZONE_DEG ? (Number.isFinite(nativeHeadingDeg) ? Math.min(0.42, Math.max(0.12, absDiff / 105.0)) : Math.min(0.22, Math.max(0.06, absDiff / 190.0))) : USER_ORIENTATION_SMALL_ERROR_GAIN;
            const step = Math.max(-USER_ORIENTATION_MAX_STEP_DEG, Math.min(USER_ORIENTATION_MAX_STEP_DEG, diff * gain));
            userHeadingDeg = normalizeAngle(userHeadingDeg + step);
        }
    }

    if (!Number.isFinite(userHeadingDisplayDeg)) {
        userHeadingDisplayDeg = userHeadingDeg;
    }

    if (followUserEnabled && orientationMode === "user") {
        userHeadingIndicatorDeg = userHeadingDisplayDeg;
    } else if (!Number.isFinite(userHeadingIndicatorDeg)) {
        userHeadingIndicatorDeg = userHeadingDisplayDeg;
    }

    scheduleHeadingAnimation();
}

function setGroundMapUserHeading(deg) {
    if (!Number.isFinite(deg)) return;
    nativeHeadingDeg = normalizeAngle(deg);
    applyFusedHeading();
}

function stopBrowserHeadingSyncLoop() {
    if (browserHeadingSyncTimer == null) return;
    try {
        clearTimeout(browserHeadingSyncTimer);
    } catch (e) {
    }
    browserHeadingSyncTimer = null;
}

function scheduleBrowserHeadingSyncLoop() {
    if (browserHeadingSyncTimer != null) return;
    const step = safeMapCallback("browser heading sync", () => {
        browserHeadingSyncTimer = null;
        if (!Number.isFinite(deviceHeadingDeg)) {
            return;
        }
        applyFusedHeading();
        browserHeadingSyncTimer = setTimeout(step, BROWSER_HEADING_SYNC_INTERVAL_MS);
    });
    browserHeadingSyncTimer = setTimeout(step, BROWSER_HEADING_SYNC_INTERVAL_MS);
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
    scheduleBrowserHeadingSyncLoop();
}

function initCompassOnce() {
    if (compassInitialized) return;
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
            compassInitialized = true;
            window.addEventListener("deviceorientation", safeMapCallback("device orientation", handleOrientation));
            return;
        }
        if (saved === "denied") return;
        if (window.__gs26_compass_permission_request_allowed !== true) return;

        compassInitialized = true;
        Dev.requestPermission()
            .then((value) => {
                try {
                    if (window.localStorage) window.localStorage.setItem(key, value || "denied");
                } catch (e) {
                }
                if (value === "granted") {
                    window.addEventListener("deviceorientation", safeMapCallback("device orientation", handleOrientation));
                }
            })
            .catch(() => {
                try {
                    if (window.localStorage) window.localStorage.setItem(key, "denied");
                } catch (e) {
                }
            });
    } else {
        compassInitialized = true;
        window.addEventListener("deviceorientation", safeMapCallback("device orientation", handleOrientation));
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
    const visualTarget = currentUserMarkerVisualLatLng() || lastUserLatLng;
    scheduleFollowCameraUpdate(visualTarget);
}

function unlockMapInteraction(options) {
    const force = !!(options && options.force);
    const dropFollow = options && Object.prototype.hasOwnProperty.call(options, "dropFollow") ? !!options.dropFollow : true;
    const dropOrientation = !!(options && options.dropOrientation);
    let changed = false;
    if (dropFollow && followUserEnabled) {
        followUserEnabled = false;
        followEnableGuardUntilMs = 0;
        followCameraCenterLocked = false;
        changed = true;
    }
    if (dropOrientation && (force || orientationMode !== "manual")) {
        orientationMode = "manual";
        pendingUserUpRealign = false;
        changed = true;
    }
    if (!changed) return;

    suppressFollowDisableUntilMs = 0;
    syncWindowMapControlState();
    persistMapState();
    try {
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
        const requested = window.__gs26_follow_user_enabled;
        if (requested != null) {
            const enabled = String(requested) === "true";
            const guard = Number(window.__gs26_follow_user_enable_guard_until || 0);
            followUserEnabled = enabled || (Number.isFinite(guard) && guard > Date.now());
        }

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
    cancelSmoothWheelRotation();
    const previousMode = orientationMode;
    orientationMode = mode === "user" ? "user" : (mode === "manual" ? "manual" : "north");
    pendingUserUpRealign = orientationMode === "user";
    if (orientationMode === "north") {
        mapBearingDeg = 0;
        pendingUserUpRealign = false;
    } else if (orientationMode === "user" && followUserEnabled) {
        const headingTarget = fusedHeadingTarget();
        if (Number.isFinite(headingTarget)) {
            userHeadingDegRaw = headingTarget;
        }
        applyKnownUserUpBearingNow();
    }
    syncWindowMapControlState();
    persistMapState();
    applyMapOrientation({
        animate: previousMode !== orientationMode || orientationMode === "north",
        durationMs: ORIENTATION_MODE_ANIMATION_MS,
    });
    scheduleHeadingAnimation();
    updateCenterControlAppearance();
}

function enterManualOrientationMode() {
    if (orientationMode !== "manual") {
        orientationMode = "manual";
        pendingUserUpRealign = false;
        syncWindowMapControlState();
    }
    updateCenterControlAppearance();
}

function wheelDeltaPixels(event, value) {
    const delta = Number(value);
    if (!Number.isFinite(delta)) return 0;
    if (event && event.deltaMode === 1) return delta * 16;
    if (event && event.deltaMode === 2) return delta * 120;
    return delta;
}

function wheelRotationDeltaDeg(event) {
    if (!event) return 0;
    const deltaX = wheelDeltaPixels(event, event.deltaX);
    const deltaY = wheelDeltaPixels(event, event.deltaY);
    if (event.shiftKey && Math.abs(deltaY) >= 1) {
        return deltaY * WHEEL_ROTATE_DEG_PER_PIXEL;
    }
    if (horizontalWheelDominates(deltaX, deltaY)) {
        return deltaX * WHEEL_ROTATE_DEG_PER_PIXEL;
    }
    return 0;
}

function wheelGestureIntent(event) {
    if (!event) return null;
    const now = Date.now();
    if (wheelGestureMode && now - wheelGestureLastAtMs <= WHEEL_GESTURE_LOCK_MS) {
        wheelGestureLastAtMs = now;
        return wheelGestureMode;
    }

    const deltaX = wheelDeltaPixels(event, event.deltaX);
    const deltaY = wheelDeltaPixels(event, event.deltaY);
    let nextMode = null;
    if (event.shiftKey && Math.abs(deltaY) >= 1) {
        nextMode = "rotate";
    } else if (verticalWheelDominates(deltaX, deltaY) || wheelZoomWouldExceedLimit(deltaY)) {
        nextMode = "zoom";
    } else if (horizontalWheelDominates(deltaX, deltaY)) {
        nextMode = "rotate";
    }

    wheelGestureMode = nextMode;
    wheelGestureLastAtMs = nextMode ? now : 0;
    return nextMode;
}

function horizontalWheelDominates(deltaX, deltaY) {
    const absX = Math.abs(deltaX);
    const absY = Math.abs(deltaY);
    return absX >= 1 && absX >= Math.max(1, absY * WHEEL_ROTATE_AXIS_DOMINANCE);
}

function verticalWheelDominates(deltaX, deltaY) {
    const absX = Math.abs(deltaX);
    const absY = Math.abs(deltaY);
    return absY >= 1 && absY > absX;
}

function wheelZoomWouldExceedLimit(deltaY) {
    if (!groundMap || !Number.isFinite(deltaY) || Math.abs(deltaY) < 1) return false;
    const zoom = groundMap.getZoom();
    if (!Number.isFinite(zoom)) return false;
    if (deltaY < 0 && Number.isFinite(currentMaxZoom)) {
        return zoom >= currentMaxZoom - WHEEL_ZOOM_LIMIT_EPSILON;
    }
    if (deltaY > 0) {
        return zoom <= effectiveMinZoom() + WHEEL_ZOOM_LIMIT_EPSILON;
    }
    return false;
}

function cancelSmoothWheelRotation() {
    if (wheelRotateFrame != null) {
        cancelAnimationFrame(wheelRotateFrame);
        wheelRotateFrame = null;
    }
    wheelRotateTargetBearing = null;
}

function stepSmoothWheelRotation() {
    try {
        wheelRotateFrame = null;
        if (!groundMap || !Number.isFinite(wheelRotateTargetBearing)) {
            cancelSmoothWheelRotation();
            return;
        }

        const currentBearing = normalizeAngle(groundMap.getBearing());
        const diff = shortestAngleDiff(currentBearing, wheelRotateTargetBearing);
        const settled = Math.abs(diff) <= WHEEL_ROTATE_SETTLE_DEG;
        const nextBearing = settled ? normalizeAngle(wheelRotateTargetBearing) : normalizeAngle(currentBearing + diff * WHEEL_ROTATE_EASE);

        markInternalCameraUpdate(80);
        mapBearingDeg = nextBearing;
        try {
            groundMap.jumpTo({bearing: nextBearing});
        } catch (e) {
            cancelSmoothWheelRotation();
            return;
        }
        syncWindowMapControlState();
        updateUserMarkerRotation();
        rememberMapView();
        updateCenterControlAppearance();

        if (settled) {
            wheelRotateTargetBearing = null;
            persistMapStateSoon();
            return;
        }

        wheelRotateFrame = requestAnimationFrame(stepSmoothWheelRotation);
    } catch (error) {
        reportMapRuntimeError("wheel rotation frame", error);
        cancelSmoothWheelRotation();
    }
}

function rotateMapFromWheel(deltaDeg) {
    const delta = Number(deltaDeg);
    if (!groundMap || !Number.isFinite(delta)) return;
    const baseBearing = Number.isFinite(wheelRotateTargetBearing) ? wheelRotateTargetBearing : normalizeAngle(groundMap.getBearing());
    wheelRotateTargetBearing = normalizeAngle(baseBearing + delta);
    if (wheelRotateFrame == null) {
        wheelRotateFrame = requestAnimationFrame(stepSmoothWheelRotation);
    }
}

function adjustGroundMapBearing(deltaDeg) {
    const delta = Number(deltaDeg);
    if (!Number.isFinite(delta)) return;
    cancelSmoothWheelRotation();
    orientationMode = "manual";
    pendingUserUpRealign = false;
    mapBearingDeg = normalizeAngle(mapBearingDeg + delta);
    syncWindowMapControlState();
    persistMapState();
    applyMapOrientation();
}

function setGroundMapBearing(deg) {
    if (!Number.isFinite(deg)) return;
    cancelSmoothWheelRotation();
    orientationMode = "manual";
    pendingUserUpRealign = false;
    mapBearingDeg = normalizeAngle(deg);
    syncWindowMapControlState();
    persistMapState();
    applyMapOrientation();
}

function setGroundMapFollowUser(enabled) {
    followUserEnabled = enabled === true;
    if (!followUserEnabled) {
        pendingUserUpRealign = false;
        followCameraCenterLocked = false;
    }
    followEnableGuardUntilMs = followUserEnabled ? Date.now() + 5000 : 0;
    syncWindowMapControlState();
    try {
        window.dispatchEvent(new CustomEvent("gs26-follow-user-changed", {
            detail: {enabled: followUserEnabled},
        }));
    } catch (e) {
    }
    if (followUserEnabled && orientationMode === "user") {
        applyKnownUserUpBearingNow();
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
    snapFollowCameraTo(currentUserMarkerVisualLatLng() || lastUserLatLng);
    return true;
}

function activateLocateControl() {
    if (!followUserEnabled) {
        setGroundMapFollowUser(true);
        centerOnUserNow();
        return;
    }

    if (orientationMode !== "user" && hasUsableUserHeading()) {
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

function isGroundMapVisibleForPrefetch() {
    if (!groundMap || typeof document === "undefined") return false;
    if (document.visibilityState && document.visibilityState !== "visible") return false;
    try {
        const container = groundMap.getContainer ? groundMap.getContainer() : null;
        if (!container) return false;
        return (container.clientWidth || 0) > 0 && (container.clientHeight || 0) > 0;
    } catch (e) {
        return false;
    }
}

function shouldRunBrowserMapPrefetch() {
    return true;
}

function shouldRunHighResBrowserMapPrefetch() {
    if (isGroundMapVisibleForPrefetch() && (isBrowserHostedMapRuntime() || isIosPlatform() || isAndroidPlatform())) {
        return false;
    }
    return true;
}

function highResPrefetchConcurrencyLimit() {
    if (isBrowserHostedMapRuntime()) return HIGH_RES_PREFETCH_CONCURRENCY_WEB;
    if (isDesktopNativeMapRuntime()) return HIGH_RES_PREFETCH_CONCURRENCY_NATIVE_DESKTOP;
    return HIGH_RES_PREFETCH_CONCURRENCY;
}

function trackingPrefetchIntervalMs() {
    return isBrowserHostedMapRuntime() ? TRACKING_PREFETCH_INTERVAL_MS_WEB : TRACKING_PREFETCH_INTERVAL_MS;
}

function trackingPrefetchDelayMs() {
    return isBrowserHostedMapRuntime() ? TRACKING_PREFETCH_DELAY_MS_WEB : TRACKING_PREFETCH_DELAY_MS;
}

function trackingPrefetchConcurrencyLimit() {
    return (isBrowserHostedMapRuntime() || isDesktopNativeMapRuntime()) ? TRACKING_PREFETCH_CONCURRENCY_WEB : TRACKING_PREFETCH_CONCURRENCY;
}

function highResPrefetchIdleDelayMs() {
    if (isBrowserHostedMapRuntime()) return HIGH_RES_PREFETCH_IDLE_DELAY_MS_WEB;
    if (isDesktopNativeMapRuntime()) return HIGH_RES_PREFETCH_IDLE_DELAY_MS_NATIVE_DESKTOP;
    return HIGH_RES_PREFETCH_IDLE_DELAY_MS;
}

function worldSizeAtZoom(zoom) {
    return 256 * Math.pow(2, Math.max(MIN_ZOOM, Math.floor(Number(zoom) || MIN_ZOOM)));
}

function latLonToWorldPointAtZoom(lat, lon, zoom) {
    const z = Math.max(MIN_ZOOM, Math.floor(Number(zoom) || MIN_ZOOM));
    const scale = worldSizeAtZoom(z);
    const clampedLat = clampLat(Number(lat));
    const clampedLon = clampLon(Number(lon));
    const sinLat = Math.sin((clampedLat * Math.PI) / 180.0);
    return {
        x: ((clampedLon + 180.0) / 360.0) * scale,
        y: (0.5 - Math.log((1 + sinLat) / (1 - sinLat)) / (4 * Math.PI)) * scale,
    };
}

function worldPointToLatLonAtZoom(x, y, zoom) {
    const z = Math.max(MIN_ZOOM, Math.floor(Number(zoom) || MIN_ZOOM));
    const scale = worldSizeAtZoom(z);
    const lon = (Number(x) / scale) * 360.0 - 180.0;
    const n = Math.PI - (2.0 * Math.PI * Number(y)) / scale;
    const lat = (180.0 / Math.PI) * Math.atan(0.5 * (Math.exp(n) - Math.exp(-n)));
    return [clampLat(lat), clampLon(lon)];
}

function makeMapStyle(tilesUrl, effectiveMaxNativeZoom) {
    const rasterTemplate = shouldUseNativeTileTemplate(tilesUrl) ? String(tilesUrl || "") : tileProtocolTemplate();
    const sourceMaxZoom = Math.max(effectiveMinZoom(), MAX_NATIVE_ZOOM_LIMIT);
    return {
        version: 8, sources: {
            [TILE_SOURCE_ID]: {
                type: "raster",
                tiles: [rasterTemplate],
                tileSize: 256,
                bounds: [NA_BOUNDS.lonMin, NA_BOUNDS.latMin, NA_BOUNDS.lonMax, NA_BOUNDS.latMax],
                minzoom: effectiveMinZoom(),
                maxzoom: sourceMaxZoom,
            }, [GUIDE_SOURCE_ID]: {
                type: "geojson", data: emptyFeatureCollection(),
            }, [ROCKET_SOURCE_ID]: {
                type: "geojson", data: emptyFeatureCollection(),
            }, [USER_SOURCE_ID]: {
                type: "geojson", data: emptyFeatureCollection(),
            }, [USER_HEADING_SOURCE_ID]: {
                type: "geojson", data: emptyFeatureCollection(),
            },
        }, layers: [{
            id: MAP_BACKGROUND_LAYER_ID, type: "background", paint: {
                "background-color": MAP_BACKGROUND_COLOR,
            },
        }, {
            id: TILE_LAYER_ID, type: "raster", source: TILE_SOURCE_ID, paint: {
                "raster-opacity": 1,
            },
        }, {
            id: GUIDE_LAYER_ID, type: "line", source: GUIDE_SOURCE_ID, paint: {
                "line-color": "#ef4444", "line-width": 3, "line-opacity": 0.95,
            },
        }, {
            id: ROCKET_LAYER_ID, type: "symbol", source: ROCKET_SOURCE_ID, layout: {
                "icon-image": ROCKET_ICON_IMAGE_ID,
                "icon-size": 0.8,
                "icon-allow-overlap": true,
                "icon-ignore-placement": true,
            },
        }, {
            id: USER_LAYER_ID, type: "symbol", source: USER_SOURCE_ID, layout: {
                "icon-image": USER_ICON_IMAGE_ID,
                "icon-size": 1.3,
                "icon-allow-overlap": true,
                "icon-ignore-placement": true,
            },
        }, {
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
        },],
    };
}

function addOverlayControls() {
    if (!groundMap || mapNavigationControl) return;
    const maplibre = getMapLibre();
    mapNavigationControl = new maplibre.NavigationControl({
        showZoom: true, showCompass: false, visualizePitch: false,
    });
    groundMap.addControl(mapNavigationControl, "top-right");
    installImmediateZoomButtonHandlers();

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
            button.addEventListener("click", safeMapCallback("center control click", (event) => {
                event.preventDefault();
                activateLocateControl();
            }));

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
            button.addEventListener("click", safeMapCallback("north control click", (event) => {
                event.preventDefault();
                setGroundMapOrientationMode("north");
                updateNorthControlAppearance();
            }));

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
        touchRotateLatched: false,
        touchCarryId: null,
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

    const rotateFromWheel = (event) => {
        if (!groundMap || event.__gs26WheelRotateHandled) return;
        const intent = wheelGestureIntent(event);
        if (intent === "zoom") {
            return;
        }
        if (intent !== "rotate") {
            return;
        }
        const deltaDeg = wheelRotationDeltaDeg(event);
        event.__gs26WheelRotateHandled = true;
        event.preventDefault();
        event.stopPropagation();
        if (typeof event.stopImmediatePropagation === "function") {
            event.stopImmediatePropagation();
        }
        if (!Number.isFinite(deltaDeg) || Math.abs(deltaDeg) < 0.01) return;
        suppressHighResPrefetch(1200);
        unlockMapInteraction({force: true, dropFollow: true, dropOrientation: true});
        enterManualOrientationMode();
        rotateMapFromWheel(deltaDeg);
        persistMapStateSoon();
        updateCenterControlAppearance();
    };

    const zoomFollowFromWheel = (event) => {
        if (!groundMap || event.__gs26WheelRotateHandled) return;
        const intent = wheelGestureIntent(event);
        if (intent !== "zoom" || !followUserEnabled) return;
        const deltaY = wheelDeltaPixels(event, event.deltaY);
        if (!applyFollowWheelZoom(deltaY)) return;
        event.preventDefault();
        event.stopPropagation();
        if (typeof event.stopImmediatePropagation === "function") {
            event.stopImmediatePropagation();
        }
    };

    canvas.addEventListener("wheel", safeMapCallback("map wheel rotate", rotateFromWheel), {
        capture: true, passive: false, signal
    });
    canvas.addEventListener("wheel", safeMapCallback("map wheel follow zoom", zoomFollowFromWheel), {
        capture: true, passive: false, signal
    });
    try {
        const mapCanvas = groundMap.getCanvas ? groundMap.getCanvas() : null;
        if (mapCanvas && mapCanvas !== canvas) {
            mapCanvas.addEventListener("wheel", safeMapCallback("map canvas wheel rotate", rotateFromWheel), {
                capture: true, passive: false, signal
            });
            mapCanvas.addEventListener("wheel", safeMapCallback("map canvas wheel follow zoom", zoomFollowFromWheel), {
                capture: true, passive: false, signal
            });
        }
    } catch (e) {
    }

    canvas.addEventListener("mousedown", safeMapCallback("map mousedown", (event) => {
        if (!groundMap || event.button !== 0) return;
        cancelSmoothWheelRotation();
        if (!event.shiftKey) {
            unlockMapInteraction({force: true, dropFollow: true, dropOrientation: false});
            return;
        }
        event.preventDefault();
        unlockMapInteraction({force: true, dropFollow: true, dropOrientation: true});
        enterManualOrientationMode();
        state.shiftRotateActive = true;
        state.shiftRotateStartX = event.clientX;
        state.shiftRotateStartBearing = mapBearingDeg;
        state.dragPanWasEnabled = !!(groundMap.dragPan && groundMap.dragPan.isEnabled());
        if (state.dragPanWasEnabled) {
            groundMap.dragPan.disable();
        }
    }), {signal});

    window.addEventListener("mousemove", safeMapCallback("map mousemove", (event) => {
        if (!state.shiftRotateActive || !groundMap) return;
        const dx = event.clientX - state.shiftRotateStartX;
        mapBearingDeg = normalizeAngle(state.shiftRotateStartBearing + dx * 0.45);
        applyMapOrientation();
    }), {signal});

    window.addEventListener("mouseup", safeMapCallback("map mouseup", () => {
        stopShiftRotate();
    }), {signal});

    canvas.addEventListener("touchstart", safeMapCallback("map touchstart", (event) => {
        if (!groundMap) return;
        if (event.touches.length === 1) {
            unlockMapInteraction({force: true, dropFollow: true, dropOrientation: false});
            state.touchGesture = null;
            state.touchCarryId = event.touches[0] ? event.touches[0].identifier : null;
            return;
        }
        if (event.touches.length !== 2) {
            state.touchGesture = null;
            if (event.touches.length === 0) {
                state.touchRotateLatched = false;
                state.touchCarryId = null;
            }
            return;
        }
        event.preventDefault();
        const carryId = state.touchCarryId;
        const continueLatchedRotation = state.touchRotateLatched && carryId != null && Array.from(event.touches).some((touch) => touch.identifier === carryId);
        state.touchGesture = {
            startAngle: touchAngle(event.touches),
            startDistance: Math.max(1, touchDistance(event.touches)),
            startMidpoint: touchMidpoint(event.touches),
            startCenter: groundMap.getCenter(),
            startZoom: groundMap.getZoom(),
            startBearing: mapBearingDeg,
            rotationUnlocked: continueLatchedRotation,
        };
    }), {passive: false, signal});

    canvas.addEventListener("touchmove", safeMapCallback("map touchmove", (event) => {
        if (!groundMap || !state.touchGesture || event.touches.length !== 2) return;
        event.preventDefault();

        const currentMidpoint = touchMidpoint(event.touches);
        const midpointDx = currentMidpoint.x - state.touchGesture.startMidpoint.x;
        const midpointDy = currentMidpoint.y - state.touchGesture.startMidpoint.y;
        const startCenterPoint = groundMap.project(state.touchGesture.startCenter);
        let nextCenter = groundMap.unproject([startCenterPoint.x - midpointDx, startCenterPoint.y - midpointDy,]);

        const currentDistance = Math.max(1, touchDistance(event.touches));
        const distanceScale = Math.max(0.25, Math.min(4.0, currentDistance / state.touchGesture.startDistance));
        const nextZoom = Math.min(currentMaxZoom, Math.max(effectiveMinZoom(), state.touchGesture.startZoom + Math.log2(distanceScale)));

        const currentAngle = touchAngle(event.touches);
        const angleDelta = shortestAngleDiff(state.touchGesture.startAngle, currentAngle);
        let nextBearing = normalizeAngle(groundMap.getBearing());
        let bearingChanged = false;
        if (!state.touchGesture.rotationUnlocked && Math.abs(angleDelta) >= TWO_TOUCH_ROTATE_THRESHOLD_DEG) {
            state.touchGesture.rotationUnlocked = true;
            state.touchRotateLatched = true;
        }
        if (state.touchGesture.rotationUnlocked) {
            unlockMapInteraction({force: true, dropFollow: true, dropOrientation: true});
            enterManualOrientationMode();
            nextBearing = normalizeAngle(state.touchGesture.startBearing - angleDelta);
            bearingChanged = true;
        } else if (followUserEnabled) {
            const followCenter = currentFollowUserZoomCenter();
            if (followCenter) {
                nextCenter = {lng: followCenter[0], lat: followCenter[1]};
                cancelFollowCameraAnimation();
            }
        }

        if (bearingChanged) {
            mapBearingDeg = nextBearing;
        }
        markInternalCameraUpdate(16);
        groundMap.jumpTo({
            center: [nextCenter.lng, nextCenter.lat], zoom: nextZoom, bearing: nextBearing,
        });
        updateUserMarkerRotation();
        rememberMapView();
        updateCenterControlAppearance();
    }), {passive: false, signal});

    const clearTouchGesture = (event) => {
        state.touchGesture = null;
        const touches = event && event.touches ? event.touches : [];
        if (touches.length === 0) {
            state.touchRotateLatched = false;
            state.touchCarryId = null;
            return;
        }
        if (touches.length === 1) {
            state.touchCarryId = touches[0].identifier;
            return;
        }
        state.touchCarryId = null;
    };
    canvas.addEventListener("touchend", safeMapCallback("map touchend", clearTouchGesture), {signal});
    canvas.addEventListener("touchcancel", safeMapCallback("map touchcancel", clearTouchGesture), {signal});

}

function installMapHooks() {
    if (!groundMap) return;
    const onMap = (eventName, label, fn) => {
        groundMap.on(eventName, safeMapCallback(label, fn));
    };

    onMap("error", "map error", (event) => {
        reportMapRuntimeError("maplibre error", event && (event.error || event.message || event));
    });

    onMap("load", "map load", () => {
        pushMapTrace("map:event:load");
        logMapInitTiming("load");
        ensureMapMarkerImages();
        mapReady = true;
        restorePendingZoomIfPossible();
        updateZoomControlAppearance();
        syncRocketGuideLine(lastRocketLatLng, currentUserVisualOrLastLatLng());
        syncPointSource(ROCKET_SOURCE_ID, lastRocketLatLng);
        syncPointSource(USER_SOURCE_ID, currentUserVisualOrLastLatLng());
        syncUserHeadingIndicator();
        setTimeout(safeMapCallback("map load deferred prefetch", () => {
            scheduleTrackingTilePrefetch();
            scheduleHighResTilePrefetch();
            scheduleTileZoomDiscovery();
        }), STARTUP_CACHE_BYPASS_MS);
        stopMapMainThreadWatchdog("map-load");
    });

    onMap("moveend", "map moveend", () => {
        rememberMapView();
        persistMapStateSoon();
        scheduleTileZoomDiscovery();
        updateZoomControlAppearance();
    });
    onMap("move", "map move", () => {
        scheduleTrackingTilePrefetch();
    });
    onMap("zoomend", "map zoomend", () => {
        try {
            const zoom = groundMap && typeof groundMap.getZoom === "function" ? groundMap.getZoom() : NaN;
            if (!Number.isFinite(zoomButtonTargetZoom) || (Number.isFinite(zoom) && Math.abs(zoom - zoomButtonTargetZoom) <= WHEEL_ZOOM_LIMIT_EPSILON)) {
                zoomButtonTargetZoom = null;
            }
        } catch (e) {
            zoomButtonTargetZoom = null;
        }
        suppressFollowCameraUntilMs = 0;
        suppressHighResPrefetch(300);
        try {
            const zoom = groundMap && typeof groundMap.getZoom === "function" ? groundMap.getZoom() : NaN;
            if (promoteNativeZoomForDisplayZoom(zoom, currentTilesUrl) && groundMap && typeof groundMap.setMaxZoom === "function") {
                groundMap.setMaxZoom(currentMaxZoom);
            }
        } catch (e) {
        }
        rememberMapView();
        persistMapStateSoon();
        scheduleTrackingTilePrefetch();
        if (followUserEnabled) {
            applyFollowUserIfPossible();
        }
        updateZoomControlAppearance();
    });
    onMap("zoom", "map zoom", () => {
        if (isFollowZoomHolding()) {
            recenterFollowUserDuringZoom();
        }
        updateZoomControlAppearance();
    });
    onMap("rotateend", "map rotateend", () => {
        if (isInternalCameraUpdate() && orientationMode === "north") {
            mapBearingDeg = 0;
        } else if (isInternalCameraUpdate() && orientationMode === "user") {
            userHeadingCameraDeg = normalizeAngle(groundMap.getBearing());
            mapBearingDeg = userHeadingCameraDeg;
        } else {
            mapBearingDeg = normalizeAngle(groundMap.getBearing());
        }
        if (isInternalCameraUpdate()) {
            orientationModeAnimationUntilMs = 0;
            orientationModeSettleUntilMs = Math.max(orientationModeSettleUntilMs, Date.now() + 180);
        }
        if (!isInternalCameraUpdate() && Date.now() >= suppressManualOrientationDropUntilMs && orientationMode !== "manual") {
            orientationMode = "manual";
            pendingUserUpRealign = false;
        }
        syncWindowMapControlState();
        updateUserMarkerRotation();
        rememberMapView();
        persistMapStateSoon();
        updateCenterControlAppearance();
    });
    onMap("rotate", "map rotate", () => {
        if (!groundMap || isInternalCameraUpdate()) return;
        if (Date.now() >= suppressManualOrientationDropUntilMs && orientationMode !== "manual") {
            orientationMode = "manual";
            pendingUserUpRealign = false;
        }
        mapBearingDeg = normalizeAngle(groundMap.getBearing());
        syncWindowMapControlState();
        updateUserMarkerRotation();
        updateCenterControlAppearance();
    });

    onMap("drag", "map drag", () => {
        updateUserMarkerRotation();
    });

    onMap("dragstart", "map dragstart", () => {
        cancelSmoothWheelRotation();
        suppressHighResPrefetch(300);
        unlockMapInteraction({force: true, dropFollow: true, dropOrientation: true});
    });
    onMap("zoomstart", "map zoomstart", () => {
        cancelSmoothWheelRotation();
        suppressHighResPrefetch(300);
        if (followUserEnabled && currentFollowUserZoomCenter()) {
            beginFollowZoomHold(900);
        }
        suppressFollowCameraUntilMs = Date.now() + 1000;
        suppressManualOrientationDropUntilMs = Date.now() + 1500;
        unlockMapInteraction({force: true, dropFollow: false, dropOrientation: false});
    });
    onMap("rotatestart", "map rotatestart", () => {
        if (!isInternalCameraUpdate()) {
            cancelSmoothWheelRotation();
        }
        suppressHighResPrefetch(300);
        if (isInternalCameraUpdate()) return;
        suppressFollowCameraUntilMs = Date.now() + 1500;
        unlockMapInteraction({force: true, dropFollow: true, dropOrientation: true});
    });
    for (const eventName of ["pitchstart"]) {
        onMap(eventName, `map ${eventName}`, disableFollowUserFromMapInteraction);
    }

    try {
        const canvas = groundMap.getCanvas();
        if (canvas && !canvas.__gs26_follow_disable_hooks) {
            canvas.__gs26_follow_disable_hooks = true;
            canvas.addEventListener("wheel", safeMapCallback("map wheel unlock", () => {
                unlockMapInteraction({force: true, dropFollow: false, dropOrientation: false});
            }), {passive: true});
        }
    } catch (e) {
    }
}

function resetMapObjects(options = {}) {
    const clearTileRuntimeCache = !!(options && options.clearTileRuntimeCache);
    const preservedUserVisual = currentUserMarkerVisualLatLng();
    cancelSmoothWheelRotation();
    cancelZoomButtonAnimation();
    zoomButtonTargetZoom = null;
    cancelFollowZoomAnimation();
    followZoomHoldUntilMs = 0;
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
    if (runtimeMinZoomTimer != null) {
        clearTimeout(runtimeMinZoomTimer);
        runtimeMinZoomTimer = null;
    }
    pendingRuntimeMinZoom = null;
    if (mapFirstPaintTimer != null) {
        clearTimeout(mapFirstPaintTimer);
        mapFirstPaintTimer = null;
    }
    mapFirstPaintGateActive = false;
    mapFirstPaintTargetZoom = null;
    if (clearTileRuntimeCache) {
        clearTileRuntimeCaches();
    }
    if (markerSyncTimer) {
        clearTimeout(markerSyncTimer);
        markerSyncTimer = null;
    }
    pendingMarkerSync = null;
    mapReady = false;
    mapNavigationControl = null;
    mapCenterControl = null;
    mapNorthControl = null;
    if (Array.isArray(preservedUserVisual)) {
        userMarkerDisplayedLatLng = [preservedUserVisual[0], preservedUserVisual[1]];
    }
    userHeadingIndicatorDeg = Number.isFinite(userHeadingDeg) ? userHeadingDeg : userHeadingIndicatorDeg;
}

function mapCameraAlreadyAt(center, zoom, bearing) {
    if (!groundMap || !Array.isArray(center)) return false;
    try {
        const currentCenter = groundMap.getCenter && groundMap.getCenter();
        const currentZoom = groundMap.getZoom && Number(groundMap.getZoom());
        const currentBearing = groundMap.getBearing && Number(groundMap.getBearing());
        if (!currentCenter || !Number.isFinite(currentZoom) || !Number.isFinite(currentBearing)) return false;
        return Math.abs(Number(currentCenter.lng) - Number(center[0])) <= 0.000001 && Math.abs(Number(currentCenter.lat) - Number(center[1])) <= 0.000001 && Math.abs(currentZoom - Number(zoom)) <= 0.001 && Math.abs(shortestAngleDiff(currentBearing, Number(bearing))) <= 0.05;
    } catch (e) {
        return false;
    }
}

function initGroundMap(tilesUrl, centerLat, centerLon, zoom, maxNativeZoom, assetTitle) {
    logMapRuntimeBoundary("initGroundMap:start", {
        tilesUrl, zoom, maxNativeZoom,
    });
    startMapMainThreadWatchdog("initGroundMap");
    mapInitTrace = [];
    pushMapTrace("initGroundMap:start");
    installMapRuntimeGuardsOnce();
    logMapRuntimeBoundary("initGroundMap:after-runtime-guards");
    pushMapTrace("initGroundMap:runtime-guards");
    ensureMarkerStylesOnce();
    logMapRuntimeBoundary("initGroundMap:after-marker-styles");
    pushMapTrace("initGroundMap:marker-styles");
    setTimeout(() => {
        requestPersistentTileStorage();
    }, 0);
    initCompassOnce();
    logMapRuntimeBoundary("initGroundMap:after-compass");
    pushMapTrace("initGroundMap:compass");
    if (!shouldUseNativeTileTemplate(tilesUrl)) {
        ensureMapProtocolOnce();
        logMapRuntimeBoundary("initGroundMap:after-protocol-ready");
        pushMapTrace("initGroundMap:protocol-ready");
    }
    mapInitStartedAtMs = Date.now();
    mapInitTimingLogged = false;
    firstTileTimingLogged = false;
    logMapInitTiming("init");
    pushMapTrace("initGroundMap:timing-reset");
    if (groundMap) {
        rememberMapView();
        persistMapState();
        logMapRuntimeBoundary("initGroundMap:after-persist-existing-map");
        pushMapTrace("initGroundMap:persist-existing-map");
    } else {
        loadPersistedMapState();
        logMapRuntimeBoundary("initGroundMap:after-load-persisted-state");
        pushMapTrace("initGroundMap:load-persisted-state");
    }

    const previousTilesUrl = currentTilesUrl;
    const nextConfiguredMaxNativeZoom = clampMaxNativeZoom(maxNativeZoom);
    const nextConfiguredMaxDisplayZoom = clampMaxDisplayZoom(Number(window.__gs26_max_display_zoom), nextConfiguredMaxNativeZoom);
    let nextMaxNativeZoom = effectiveMaxNativeZoomFor(nextConfiguredMaxNativeZoom, tilesUrl);
    let nextMaxZoom = Math.max(DEFAULT_SAFE_MIN_ZOOM, nextConfiguredMaxDisplayZoom, nextMaxNativeZoom + DEFAULT_MAX_OVERZOOM_DELTA);
    trackedAssetLabel = assetTitle || trackedAssetTitle();
    currentTilesUrl = tilesUrl;
    configuredMaxNativeZoom = nextConfiguredMaxNativeZoom;
    configuredMaxDisplayZoom = nextConfiguredMaxDisplayZoom;
    currentMaxNativeZoom = nextMaxNativeZoom;
    currentMaxZoom = nextMaxZoom;
    currentMinZoom = DEFAULT_SAFE_MIN_ZOOM;
    currentPrefetchKey = null;
    if (previousTilesUrl && previousTilesUrl !== tilesUrl) {
        invalidateTileCachesForUrlChange();
        pushMapTrace("initGroundMap:invalidate-url-caches");
    }

    const container = document.getElementById("ground-map");
    if (!container) {
        logMapRuntimeBoundary("initGroundMap:no-container");
        pushMapTrace("initGroundMap:no-container");
        return;
    }
    logMapRuntimeBoundary("initGroundMap:container-ready");
    pushMapTrace("initGroundMap:container-ready");

    const desiredZoom = Number.isFinite(pendingRestoreZoom) ? pendingRestoreZoom : (Number.isFinite(lastMapZoom) ? lastMapZoom : (lastMapView && Number.isFinite(lastMapView.zoom) ? lastMapView.zoom : zoom));
    usePersistedCachedZoomForStartup(tilesUrl, desiredZoom);
    promoteNativeZoomForDisplayZoom(desiredZoom, tilesUrl);
    if (Number.isFinite(desiredZoom) && desiredZoom > currentMaxZoom) {
        currentMaxZoom = Math.min(MAX_DISPLAY_ZOOM_LIMIT, desiredZoom);
    }
    const clampedZoom = Math.min(currentMaxZoom, Math.max(effectiveMinZoom(), desiredZoom));
    if (Number.isFinite(pendingRestoreZoom) && pendingRestoreZoom <= currentMaxZoom) {
        pendingRestoreZoom = null;
        lastMapZoom = clampedZoom;
    } else if (!Number.isFinite(pendingRestoreZoom)) {
        lastMapZoom = clampedZoom;
    }
    const startCenter = lastMapView ? [lastMapView.lon, lastMapView.lat] : [centerLon, centerLat];
    const startBearing = orientationMode === "north" ? 0 : mapBearingDeg;
    logMapRuntimeBoundary("initGroundMap:computed-start-state", {
        clampedZoom, useNativeTemplate: shouldUseNativeTileTemplate(currentTilesUrl),
    });
    pushMapTrace("initGroundMap:computed-start-state", {
        desiredZoom, clampedZoom, usingNativeTiles: shouldUseNativeTileTemplate(currentTilesUrl),
    });

    const needsFullRecreate = !!groundMap && (previousTilesUrl !== tilesUrl);
    persistMaxNativeZoom(tilesUrl, currentMaxNativeZoom);

    if (!needsFullRecreate && groundMap && groundMap.getContainer && groundMap.getContainer() === container) {
        logMapRuntimeBoundary("initGroundMap:maplibre-reuse:start");
        pushMapTrace("initGroundMap:reuse-maplibre");
        groundMap.resize();
        groundMap.setMaxZoom(currentMaxZoom);
        updateZoomControlAppearance();
        if (!mapCameraAlreadyAt(startCenter, clampedZoom, startBearing)) {
            markInternalCameraUpdate(250);
            groundMap.jumpTo({
                center: startCenter, zoom: clampedZoom, bearing: startBearing,
            });
        }
        rememberMapView();
        applyMapOrientation();
        applyPendingCenterIfPossible();
        applyFollowUserIfPossible();
        scheduleTileZoomDiscovery();
        logMapRuntimeBoundary("initGroundMap:maplibre-reuse:complete");
        pushMapTrace("initGroundMap:reuse-maplibre-complete");
        return;
    }

    if (groundMap) {
        logMapRuntimeBoundary("initGroundMap:remove-existing-map");
        pushMapTrace("initGroundMap:remove-existing-map");
        try {
            groundMap.remove();
        } catch (e) {
        }
        groundMap = null;
        window.__gs26_ground_map = null;
    }
    resetMapObjects({clearTileRuntimeCache: previousTilesUrl && previousTilesUrl !== currentTilesUrl});
    scheduleCachedZoomDiscoveryAfterStartup(currentTilesUrl);
    if (!shouldUseNativeTileTemplate(currentTilesUrl)) {
        logMapRuntimeBoundary("initGroundMap:first-paint-gate:start");
        pushMapTrace("initGroundMap:first-paint-gate:start");
        startMapFirstPaintGate(clampedZoom);
        renderAfterStartupCacheWarm(warmInitialMapTilesFromCache(currentTilesUrl, startCenter, clampedZoom, container));
    } else {
        logMapRuntimeBoundary("initGroundMap:first-paint-gate:skip");
        pushMapTrace("initGroundMap:first-paint-gate:skip");
        finishMapFirstPaintGate("browser-direct-tiles");
    }

    let maplibre = null;
    try {
        logMapRuntimeBoundary("initGroundMap:maplibre-ctor:pre-get");
        pushMapTrace("initGroundMap:maplibre-ctor:start");
        maplibre = getMapLibre();
        logMapRuntimeBoundary("initGroundMap:maplibre-ctor:pre-new");
        groundMap = new maplibre.Map({
            container,
            style: makeMapStyle(currentTilesUrl, currentMaxNativeZoom),
            center: startCenter,
            zoom: clampedZoom,
            bearing: startBearing,
            minZoom: effectiveMinZoom(),
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
            fadeDuration: 0,
            refreshExpiredTiles: false,
        });
        logMapRuntimeBoundary("initGroundMap:maplibre-ctor:post-new");
        pushMapTrace("initGroundMap:maplibre-ctor:done");
    } catch (error) {
        logMapRuntimeBoundary("initGroundMap:maplibre-ctor:error", {
            error: String(error && (error.message || error) || ""),
        });
        pushMapTrace("initGroundMap:maplibre-ctor:error", {
            error: String(error && (error.message || error) || ""),
        });
        reportMapRuntimeError("map construction failed", error);
        finishMapFirstPaintGate("map-construction-error");
        groundMap = null;
        window.__gs26_ground_map = null;
        return;
    }
    groundMap.invalidateSize = () => {
        try {
            groundMap.resize();
        } catch (e) {
        }
    };
    logMapRuntimeBoundary("initGroundMap:after-maplibre-wrap");
    if (groundMap.dragPan && !groundMap.dragPan.isEnabled()) {
        groundMap.dragPan.enable();
    }
    if (groundMap.scrollZoom && !groundMap.scrollZoom.isEnabled()) {
        groundMap.scrollZoom.enable();
    }
    if (groundMap.touchZoomRotate && typeof groundMap.touchZoomRotate.enable === "function") {
        groundMap.touchZoomRotate.enable();
        if (typeof groundMap.touchZoomRotate.disableRotation === "function") {
            groundMap.touchZoomRotate.disableRotation();
        }
    }
    if (groundMap.touchPitch && typeof groundMap.touchPitch.disable === "function") {
        groundMap.touchPitch.disable();
    }
    addOverlayControls();
    logMapRuntimeBoundary("initGroundMap:after-addOverlayControls");
    pushMapTrace("initGroundMap:add-overlay-controls");
    updateZoomControlAppearance();
    installMapHooks();
    logMapRuntimeBoundary("initGroundMap:after-installMapHooks");
    pushMapTrace("initGroundMap:install-map-hooks");
    installCustomGestureHooks();
    logMapRuntimeBoundary("initGroundMap:after-installCustomGestureHooks");
    pushMapTrace("initGroundMap:install-gesture-hooks");
    rememberMapView();
    window.__gs26_ground_map = groundMap;
    syncRequestedMapControlState();

    if (Array.isArray(userMarkerDisplayedLatLng)) {
        userMarkerDisplayedLatLng = [userMarkerDisplayedLatLng[0], userMarkerDisplayedLatLng[1]];
    } else if (lastUserLatLng) {
        userMarkerDisplayedLatLng = [lastUserLatLng[0], lastUserLatLng[1]];
    }

    syncPointSource(ROCKET_SOURCE_ID, lastRocketLatLng);
    syncPointSource(USER_SOURCE_ID, currentUserVisualOrLastLatLng());
    syncUserHeadingIndicator();

    applyPendingCenterIfPossible();
    applyFollowUserIfPossible();
    applyMapOrientation();
    updateCenterControlAppearance();
    logMapRuntimeBoundary("initGroundMap:complete");
    pushMapTrace("initGroundMap:complete");
}

function applyGroundMapMarkers(rLat, rLon, uLat, uLon) {
    if (markerSyncTimer != null) {
        clearTimeout(markerSyncTimer);
        markerSyncTimer = null;
    }
    markerSyncTimer = null;
    pendingMarkerSync = null;
    lastMarkerSyncAtMs = Date.now();

    const hasRocket = isUsableLatLng(rLat, rLon);
    const hasUser = isUsableLatLng(uLat, uLon);

    if (hasRocket) {
        lastRocketLatLng = [rLat, rLon];
        prefetchRocketLatLng = [rLat, rLon];
    } else {
        lastRocketLatLng = null;
        prefetchRocketLatLng = null;
        rocketGpsStability = null;
        try {
            delete window.__gs26_rocket_lat;
            delete window.__gs26_rocket_lon;
        } catch (e) {
            window.__gs26_rocket_lat = NaN;
            window.__gs26_rocket_lon = NaN;
        }
    }
    if (hasUser) {
        lastUserLatLng = [uLat, uLon];
        prefetchUserLatLng = [uLat, uLon];
        try {
            window.__gs26_user_lat = uLat;
            window.__gs26_user_lon = uLon;
        } catch (e) {
        }
    } else {
        prefetchUserLatLng = null;
    }
    publishTilePrefetchContextState();
    if (!groundMap) {
        if (hasRocket || hasUser) {
            scheduleTrackingTilePrefetch();
        }
        return;
    }

    syncPointSource(ROCKET_SOURCE_ID, hasRocket ? lastRocketLatLng : null);

    if (hasUser) {
        let userMarkerCreated = false;
        if (!userMarkerHasLiveFix || !Array.isArray(userMarkerDisplayedLatLng)) {
            const seedUserLatLng = lastUserLatLng;
            resetUserMotionSmoothing(seedUserLatLng);
            userMarkerHasLiveFix = true;
            userMarkerCreated = true;
        } else {
            animateUserMarkerTo(lastUserLatLng);
        }
        syncRequestedMapControlState();
        if (userMarkerCreated && followUserEnabled) {
            applyFollowUserIfPossible();
        }
    }

    syncRocketGuideLine(hasRocket ? lastRocketLatLng : null, hasUser ? currentUserVisualOrLastLatLng() : null);
    persistMapStateSoon();
    applyPendingCenterIfPossible();
    applyMapOrientation();
    if (hasRocket || hasUser) {
        scheduleTileZoomDiscovery();
        scheduleTrackingTilePrefetch();
    }
}

function updateGroundMapMarkers(rLat, rLon, uLat, uLon) {
    const hasRocketInput = isUsableLatLng(rLat, rLon);
    const nextRocket = hasRocketInput ? stabilizeLatLng("rocket", rLat, rLon) : null;
    if (!hasRocketInput) {
        rocketGpsStability = null;
    }
    const nextUser = stabilizeLatLng("user", uLat, uLon);
    prefetchRocketLatLng = nextRocket ? [nextRocket[0], nextRocket[1]] : null;
    prefetchUserLatLng = nextUser ? [nextUser[0], nextUser[1]] : null;
    publishTilePrefetchContextState();
    refreshTilePrefetchEstimate();
    pendingMarkerSync = [nextRocket ? nextRocket[0] : NaN, nextRocket ? nextRocket[1] : NaN, nextUser ? nextUser[0] : NaN, nextUser ? nextUser[1] : NaN,];
    if (!hasRocketInput) {
        const args = pendingMarkerSync;
        applyGroundMapMarkers(args[0], args[1], args[2], args[3]);
        return;
    }
    const now = Date.now();
    const delayMs = Math.max(0, MARKER_SYNC_MIN_INTERVAL_MS - (now - lastMarkerSyncAtMs));
    if (delayMs <= 0) {
        const args = pendingMarkerSync;
        applyGroundMapMarkers(args[0], args[1], args[2], args[3]);
        return;
    }
    if (markerSyncTimer != null) return;
    markerSyncTimer = setTimeout(safeMapCallback("marker sync timer", () => {
        const args = pendingMarkerSync;
        if (!args) {
            markerSyncTimer = null;
            return;
        }
        applyGroundMapMarkers(args[0], args[1], args[2], args[3]);
    }), delayMs);
}

function centerGroundMapOn(lat, lon) {
    if (!groundMap) return;
    markInternalCameraUpdate(250);
    groundMap.jumpTo({
        center: [lon, lat], bearing: mapBearingDeg,
    });
    rememberMapView();
    scheduleTileZoomDiscovery();
}

function getLastUserLatLng() {
    if (!Array.isArray(lastUserLatLng) || !isUsableUserLatLng(lastUserLatLng[0], lastUserLatLng[1])) {
        return null;
    }
    return {lat: lastUserLatLng[0], lon: lastUserLatLng[1]};
}

function setGroundMapPrefetchContext(tilesUrl, maxNativeZoom, rocketLat, rocketLon, userLat, userLon) {
    const nextTilesUrl = String(tilesUrl || "").trim();
    if (nextTilesUrl) {
        currentTilesUrl = nextTilesUrl;
        window.__gs26_tiles_url = nextTilesUrl;
    }

    const nextMaxNativeZoom = Number(maxNativeZoom);
    if (Number.isFinite(nextMaxNativeZoom)) {
        currentMaxNativeZoom = clampMaxNativeZoom(nextMaxNativeZoom);
        window.__gs26_max_native_zoom = currentMaxNativeZoom;
        persistMaxNativeZoom(effectivePrefetchTilesUrl(), currentMaxNativeZoom);
    }

    const rLat = Number(rocketLat);
    const rLon = Number(rocketLon);
    const uLat = Number(userLat);
    const uLon = Number(userLon);
    const stableRocket = isUsableLatLng(rLat, rLon) ? stabilizeLatLng("rocket", rLat, rLon) : null;
    const stableUser = stabilizeLatLng("user", uLat, uLon);
    prefetchRocketLatLng = stableRocket ? [stableRocket[0], stableRocket[1]] : null;
    prefetchUserLatLng = stableUser ? [stableUser[0], stableUser[1]] : null;
    if (stableRocket) {
        lastRocketLatLng = [stableRocket[0], stableRocket[1]];
    }
    if (stableUser) {
        lastUserLatLng = [stableUser[0], stableUser[1]];
    }
    publishTilePrefetchContextState();
    refreshTilePrefetchEstimate();

    if (stableUser) {
        scheduleTrackingTilePrefetch();
        if (shouldRunAutomaticHighResPrefetch()) {
            scheduleHighResTilePrefetch();
        }
    }
}

(function pinGroundStation26() {
    const api = (window.GS26 = window.GS26 || {});
    setStoredTileCacheUsageBytes(storedTileCacheUsageBytes());

    api.initGroundMap = safeMapCallback("api initGroundMap", initGroundMap);
    api.updateGroundMapMarkers = safeMapCallback("api updateGroundMapMarkers", updateGroundMapMarkers);
    api.centerGroundMapOn = safeMapCallback("api centerGroundMapOn", centerGroundMapOn);
    api.getLastUserLatLng = safeMapCallback("api getLastUserLatLng", getLastUserLatLng);
    api.scheduleHighResTilePrefetch = safeMapCallback("api scheduleHighResTilePrefetch", scheduleHighResTilePrefetch);
    api.prefetchGroundMapTiles = safeMapCallback("api prefetchGroundMapTiles", prefetchGroundMapTilesNow);
    api.setGroundMapPrefetchContext = safeMapCallback("api setGroundMapPrefetchContext", setGroundMapPrefetchContext);
    api.setGroundMapFollowUser = safeMapCallback("api setGroundMapFollowUser", setGroundMapFollowUser);
    api.setGroundMapOrientationMode = safeMapCallback("api setGroundMapOrientationMode", setGroundMapOrientationMode);
    api.disableFollowUserFromMapInteraction = safeMapCallback("api disableFollowUserFromMapInteraction", disableFollowUserFromMapInteraction);
    api.adjustGroundMapBearing = safeMapCallback("api adjustGroundMapBearing", adjustGroundMapBearing);
    api.setGroundMapBearing = safeMapCallback("api setGroundMapBearing", setGroundMapBearing);
    api.syncRequestedMapControlState = safeMapCallback("api syncRequestedMapControlState", syncRequestedMapControlState);
    api.initCompassOnce = safeMapCallback("api initCompassOnce", initCompassOnce);
    api.handleOrientation = safeMapCallback("api handleOrientation", handleOrientation);
    api.getMapLibre = getMapLibre;
    api.normalizeAngle = normalizeAngle;
    api.shortestAngleDiff = shortestAngleDiff;
    api.setTileCacheEnabled = safeMapCallback("api setTileCacheEnabled", (enabled) => {
        const nextEnabled = enabled === true;
        window.__gs26_tile_cache_enabled = nextEnabled;
        window.__gs26_tile_cache_disabled = !nextEnabled;
        try {
            if (window.localStorage) {
                window.localStorage.setItem("gs26_tile_cache_enabled", nextEnabled ? "on" : "off");
            }
        } catch (e) {
        }
        if (!nextEnabled) {
            clearTileRuntimeCaches();
            stopTrackingTilePrefetch();
            suppressHighResPrefetch(60_000);
        }
    });
    api.setCacheBudgetBytes = safeMapCallback("api setCacheBudgetBytes", (bytes) => {
        const nextBytes = Number(bytes);
        if (!Number.isFinite(nextBytes) || nextBytes <= 0) return;
        window.__gs26_cache_budget_bytes = Math.max(1, nextBytes);
        try {
            if (window.localStorage) {
                window.localStorage.setItem("gs_cache_budget_mb", String(Math.round(window.__gs26_cache_budget_bytes / 1024 / 1024)));
            }
        } catch (e) {
        }
        refreshTilePrefetchEstimate({sampleTileSize: true});
    });
    api.clearGroundMapTileCaches = safeMapCallback("api clearGroundMapTileCaches", clearAllGroundMapTileCaches);
    api.ensureMarkerStylesOnce = ensureMarkerStylesOnce;
    api.rememberMapView = safeMapCallback("api rememberMapView", rememberMapView);
    api.updateUserMarkerRotation = safeMapCallback("api updateUserMarkerRotation", updateUserMarkerRotation);
    api.setGroundMapUserHeading = safeMapCallback("api setGroundMapUserHeading", setGroundMapUserHeading);
    api.applyMapOrientation = safeMapCallback("api applyMapOrientation", applyMapOrientation);
    api.syncRocketGuideLine = safeMapCallback("api syncRocketGuideLine", syncRocketGuideLine);
    api.reloadPersistedMapState = safeMapCallback("api reloadPersistedMapState", loadPersistedMapState);
    window.__gs26_reload_persisted_map_state = api.reloadPersistedMapState;

    api.state = api.state || {};
    Object.assign(api.state, {
        NA_BOUNDS, MIN_ZOOM, DEFAULT_MAX_NATIVE_ZOOM, get groundMap() {
            return groundMap;
        }, get lastRocketLatLng() {
            return lastRocketLatLng;
        }, get lastUserLatLng() {
            return lastUserLatLng;
        }, get followUserEnabled() {
            return followUserEnabled;
        }, get orientationMode() {
            return orientationMode;
        }, get mapBearingDeg() {
            return mapBearingDeg;
        }, get lastMapView() {
            return lastMapView;
        }, get userHeadingDegRaw() {
            return userHeadingDegRaw;
        }, get userHeadingDeg() {
            return userHeadingDeg;
        }, get compassInitialized() {
            return compassInitialized;
        }, get tilePrefetchState() {
            return tilePrefetchState;
        }, get tilePrefetchEstimateState() {
            return tilePrefetchEstimateState;
        }, get tilePrefetchContextState() {
            return tilePrefetchContextState;
        },
    });

    window.initGroundMap = api.initGroundMap;
    window.updateGroundMapMarkers = api.updateGroundMapMarkers;
    window.centerGroundMapOn = api.centerGroundMapOn;
    window.getLastUserLatLng = api.getLastUserLatLng;
    window.setGroundMapPrefetchContext = api.setGroundMapPrefetchContext;
    window.initCompassOnce = api.initCompassOnce;
    window.setGroundMapUserHeading = api.setGroundMapUserHeading;
    window.setGroundMapFollowUser = api.setGroundMapFollowUser;
    window.setGroundMapOrientationMode = api.setGroundMapOrientationMode;
    window.adjustGroundMapBearing = api.adjustGroundMapBearing;
    window.setGroundMapBearing = api.setGroundMapBearing;
    window.syncRequestedMapControlState = api.syncRequestedMapControlState;
    window.scheduleHighResTilePrefetch = api.scheduleHighResTilePrefetch;
    window.prefetchGroundMapTiles = api.prefetchGroundMapTiles;
    window.setGroundMapTileCacheEnabled = api.setTileCacheEnabled;
    window.setGroundMapCacheBudgetBytes = api.setCacheBudgetBytes;
    window.clearGroundMapTileCaches = api.clearGroundMapTileCaches;

    window.__gs26_ground_station_loaded = true;
    try {
        window.dispatchEvent(new CustomEvent("gs26-ground-map-ready"));
    } catch (e) {
    }
    window.__gs26_ground_map_cache_state = {...tilePrefetchState};
    window.__gs26_ground_map_prefetch_estimate = {...tilePrefetchEstimateState};
    window.__gs26_ground_map_prefetch_context = {...publishTilePrefetchContextState()};
    window.__gs26_ground_map_cache_ready = false;
})();
