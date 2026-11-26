// Recovery search track in the background, iOS side (docs/proximity-lock.md
// "Recovery search in the background"; Rust:
// src/telemetry_dashboard/recovery_search_native.rs).
//
// Rust starts this only while the user shares their track: the station's
// search is running, the user opted in on this device for that search, and
// location is allowed. It stops it when the user stops sharing, the search
// ends, or the dashboard goes away.
//
// Its own CLLocationManager, separate from the map's LocationShim and the
// proximity lock's ProximityShim:
//  * standard location updates that continue in the background
//    (UIBackgroundModes "location", allowsBackgroundLocationUpdates) with the
//    blue location indicator always shown (showsBackgroundLocationIndicator),
//    so the user can see that the track is being recorded;
//  * "While Using" is enough: updates started from the open app continue in
//    the background. It never asks for "Always";
//  * every fix waits in a bounded buffer here until Rust takes it into the
//    search outbox (the same thinning and upload as foreground fixes).
//    Nothing here sends a location anywhere.
//
// If iOS terminates the app, recording stops; fixes Rust already took are in
// the persisted outbox and are uploaded when the app runs again.

#import <CoreLocation/CoreLocation.h>
#import <Foundation/Foundation.h>

// About 28 hours at one fix every 5 s; the oldest are dropped first.
#define GS26_SEARCH_MAX_FIXES 20000

typedef struct {
  double lat, lon, acc, unix_s;
} GS26SearchFix;

static GS26SearchFix g_fixes[GS26_SEARCH_MAX_FIXES];
static int g_head = 0;   // index of the oldest fix
static int g_count = 0;
static NSObject *g_search_lock;

@interface GS26SearchTrackShim : NSObject <CLLocationManagerDelegate>
@property(nonatomic, strong) CLLocationManager *mgr;
@property(nonatomic, assign) BOOL running;
@end

@implementation GS26SearchTrackShim

+ (instancetype)shared {
  static GS26SearchTrackShim *s;
  static dispatch_once_t once;
  dispatch_once(&once, ^{
    s = [GS26SearchTrackShim new];
    g_search_lock = [NSObject new];
  });
  return s;
}

- (CLLocationManager *)manager {
  if (!self.mgr) {
    self.mgr = [CLLocationManager new];
    self.mgr.delegate = self;
    self.mgr.desiredAccuracy = kCLLocationAccuracyBest;
    // The Rust recorder keeps a fix after 5 m or 30 s.
    self.mgr.distanceFilter = 5.0;
    self.mgr.pausesLocationUpdatesAutomatically = NO;
    self.mgr.activityType = CLActivityTypeFitness;
  }
  return self.mgr;
}

- (BOOL)authorized {
  CLAuthorizationStatus status = [self manager].authorizationStatus;
  return [CLLocationManager locationServicesEnabled] &&
         (status == kCLAuthorizationStatusAuthorizedAlways ||
          status == kCLAuthorizationStatusAuthorizedWhenInUse);
}

- (void)start {
  CLLocationManager *m = [self manager];
  if (self.running || ![self authorized]) {
    return;
  }
  m.allowsBackgroundLocationUpdates = YES;
  // The blue indicator: visible whenever this records in the background.
  m.showsBackgroundLocationIndicator = YES;
  [m startUpdatingLocation];
  self.running = YES;
}

- (void)stop {
  if (self.mgr) {
    [self.mgr stopUpdatingLocation];
    self.mgr.allowsBackgroundLocationUpdates = NO;
  }
  self.running = NO;
  @synchronized(g_search_lock) {
    g_head = 0;
    g_count = 0;
  }
}

- (void)locationManager:(CLLocationManager *)manager didUpdateLocations:(NSArray<CLLocation *> *)locations {
  if (!self.running) {
    return;
  }
  @synchronized(g_search_lock) {
    for (CLLocation *loc in locations) {
      if (loc.horizontalAccuracy < 0 || !CLLocationCoordinate2DIsValid(loc.coordinate)) {
        continue;
      }
      int slot;
      if (g_count < GS26_SEARCH_MAX_FIXES) {
        slot = (g_head + g_count) % GS26_SEARCH_MAX_FIXES;
        g_count++;
      } else {
        slot = g_head;
        g_head = (g_head + 1) % GS26_SEARCH_MAX_FIXES;
      }
      g_fixes[slot] = (GS26SearchFix){
          loc.coordinate.latitude, loc.coordinate.longitude, loc.horizontalAccuracy,
          loc.timestamp.timeIntervalSince1970};
    }
  }
}

- (void)locationManagerDidChangeAuthorization:(CLLocationManager *)manager {
  // Location turned off or denied while recording: stop; Rust sees the
  // permission and starts again once it is allowed.
  if (self.running && ![self authorized]) {
    [self stop];
  }
}

- (void)locationManager:(CLLocationManager *)manager didFailWithError:(NSError *)error {
  // kCLErrorLocationUnknown is transient; denial arrives as an authorization change.
}

@end

void gs26_search_bg_start(void) {
  dispatch_async(dispatch_get_main_queue(), ^{
    [[GS26SearchTrackShim shared] start];
  });
}

void gs26_search_bg_stop(void) {
  dispatch_async(dispatch_get_main_queue(), ^{
    [[GS26SearchTrackShim shared] stop];
  });
}

// Copies up to max_fixes of the oldest buffered fixes into out (lat, lon,
// accuracy m, Unix seconds per fix) and removes them. Returns the count.
int gs26_search_bg_take(double *out, int max_fixes) {
  [GS26SearchTrackShim shared];
  int n = 0;
  if (!out || max_fixes <= 0) {
    return 0;
  }
  @synchronized(g_search_lock) {
    while (n < max_fixes && g_count > 0) {
      GS26SearchFix f = g_fixes[g_head];
      out[n * 4 + 0] = f.lat;
      out[n * 4 + 1] = f.lon;
      out[n * 4 + 2] = f.acc;
      out[n * 4 + 3] = f.unix_s;
      g_head = (g_head + 1) % GS26_SEARCH_MAX_FIXES;
      g_count--;
      n++;
    }
  }
  return n;
}
