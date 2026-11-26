// Proximity lock, iOS side (docs/proximity-lock.md; Rust:
// src/telemetry_dashboard/proximity_native.rs).
//
// Its own CLLocationManager, separate from the map's LocationShim:
//  * asks for "When In Use" and then "Always" authorization when the user
//    agrees in the app's explanation;
//  * while the user takes part: standard location updates that continue in
//    the background (UIBackgroundModes "location", the blue indicator shown),
//    significant-change monitoring, and a CLCircularRegion around the
//    reference point (the rocket's fix or the launch site) whose entry/exit
//    wakes the app even when it was suspended;
//  * keeps the latest fix for Rust, which decides in range / out of range
//    and sends only that flag. Nothing here sends a location anywhere.
//
// If iOS terminates the app, region monitoring may relaunch it in the
// background; the Rust reporter then runs only if the app finishes
// launching. Otherwise the heartbeats stop and the station sees this device
// as unknown.

#import <CoreLocation/CoreLocation.h>
#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>
#include <stdatomic.h>

static NSString *const GS26ProximityRegionId = @"gs26.proximity";

static atomic_int g_wake = 0;
static atomic_int g_auth = 0;
static double g_lat = NAN, g_lon = NAN, g_acc = NAN, g_unix_s = NAN;
static NSObject *g_fix_lock;

@interface GS26ProximityShim : NSObject <CLLocationManagerDelegate>
@property(nonatomic, strong) CLLocationManager *mgr;
@property(nonatomic, assign) BOOL running;
@property(nonatomic, assign) BOOL wantAlways;
@end

@implementation GS26ProximityShim

+ (instancetype)shared {
  static GS26ProximityShim *s;
  static dispatch_once_t once;
  dispatch_once(&once, ^{
    s = [GS26ProximityShim new];
    g_fix_lock = [NSObject new];
  });
  return s;
}

- (CLLocationManager *)manager {
  if (!self.mgr) {
    self.mgr = [CLLocationManager new];
    self.mgr.delegate = self;
    self.mgr.desiredAccuracy = kCLLocationAccuracyNearestTenMeters;
    self.mgr.distanceFilter = 5.0;
    self.mgr.pausesLocationUpdatesAutomatically = NO;
    self.mgr.activityType = CLActivityTypeOther;
    [self refreshAuth];
  }
  return self.mgr;
}

- (void)refreshAuth {
  CLAuthorizationStatus status = self.mgr.authorizationStatus;
  int code = (int)status;
  if (![CLLocationManager locationServicesEnabled]) {
    code = 5;
  }
  atomic_store(&g_auth, code);
}

- (BOOL)authorized {
  int a = atomic_load(&g_auth);
  return a == kCLAuthorizationStatusAuthorizedAlways || a == kCLAuthorizationStatusAuthorizedWhenInUse;
}

- (void)request:(BOOL)always {
  CLLocationManager *m = [self manager];
  self.wantAlways = always;
  int a = atomic_load(&g_auth);
  if (a == kCLAuthorizationStatusNotDetermined) {
    // From iOS 13 this grants "While Using" and asks about "Always" later.
    if (always) {
      [m requestAlwaysAuthorization];
    } else {
      [m requestWhenInUseAuthorization];
    }
  } else if (a == kCLAuthorizationStatusAuthorizedWhenInUse && always) {
    // iOS shows the upgrade prompt once; after that only Settings can.
    [m requestAlwaysAuthorization];
  }
}

- (void)startAtLat:(double)lat lon:(double)lon radius:(double)radius {
  CLLocationManager *m = [self manager];
  [self refreshAuth];
  if (![self authorized]) {
    return;
  }
  m.allowsBackgroundLocationUpdates = YES;
  m.showsBackgroundLocationIndicator = YES;
  if (!self.running) {
    [m startUpdatingLocation];
    if ([CLLocationManager significantLocationChangeMonitoringAvailable]) {
      [m startMonitoringSignificantLocationChanges];
    }
    self.running = YES;
  }
  // Region monitoring needs "Always"; it is a wake-up, the decision is Rust's.
  if (atomic_load(&g_auth) == kCLAuthorizationStatusAuthorizedAlways &&
      [CLLocationManager isMonitoringAvailableForClass:[CLCircularRegion class]] &&
      CLLocationCoordinate2DIsValid(CLLocationCoordinate2DMake(lat, lon))) {
    CLLocationDistance r = MIN(MAX(radius, 100.0), m.maximumRegionMonitoringDistance);
    CLCircularRegion *region =
        [[CLCircularRegion alloc] initWithCenter:CLLocationCoordinate2DMake(lat, lon)
                                          radius:r
                                      identifier:GS26ProximityRegionId];
    region.notifyOnEntry = YES;
    region.notifyOnExit = YES;
    [m startMonitoringForRegion:region];  // replaces the one with the same id
    [m requestStateForRegion:region];
  }
}

- (void)stop {
  if (!self.mgr) {
    return;
  }
  [self.mgr stopUpdatingLocation];
  [self.mgr stopMonitoringSignificantLocationChanges];
  for (CLRegion *region in self.mgr.monitoredRegions) {
    if ([region.identifier isEqualToString:GS26ProximityRegionId]) {
      [self.mgr stopMonitoringForRegion:region];
    }
  }
  self.mgr.allowsBackgroundLocationUpdates = NO;
  self.running = NO;
}

- (void)locationManagerDidChangeAuthorization:(CLLocationManager *)manager {
  [self refreshAuth];
}

- (void)locationManager:(CLLocationManager *)manager didChangeAuthorizationStatus:(CLAuthorizationStatus)status {
  [self refreshAuth];
}

- (void)locationManager:(CLLocationManager *)manager didUpdateLocations:(NSArray<CLLocation *> *)locations {
  CLLocation *loc = locations.lastObject;
  if (!loc || loc.horizontalAccuracy < 0) {
    return;
  }
  @synchronized(g_fix_lock) {
    g_lat = loc.coordinate.latitude;
    g_lon = loc.coordinate.longitude;
    g_acc = loc.horizontalAccuracy;
    g_unix_s = loc.timestamp.timeIntervalSince1970;
  }
}

- (void)wake {
  atomic_store(&g_wake, 1);
  // A fresh fix right away; the region event itself is not the decision.
  [self.mgr requestLocation];
}

- (void)locationManager:(CLLocationManager *)manager didEnterRegion:(CLRegion *)region {
  [self wake];
}

- (void)locationManager:(CLLocationManager *)manager didExitRegion:(CLRegion *)region {
  [self wake];
}

- (void)locationManager:(CLLocationManager *)manager
      didDetermineState:(CLRegionState)state
              forRegion:(CLRegion *)region {
  [self wake];
}

- (void)locationManager:(CLLocationManager *)manager didFailWithError:(NSError *)error {
  // kCLErrorLocationUnknown is transient; denial arrives as an authorization change.
}

- (void)locationManager:(CLLocationManager *)manager
    monitoringDidFailForRegion:(CLRegion *)region
                     withError:(NSError *)error {
}

@end

int gs26_proximity_auth_status(void) {
  GS26ProximityShim *s = [GS26ProximityShim shared];
  if (!s.mgr) {
    // Read the status without creating the manager off the main thread.
    dispatch_async(dispatch_get_main_queue(), ^{
      [s manager];
    });
  }
  return atomic_load(&g_auth);
}

void gs26_proximity_request(int always) {
  dispatch_async(dispatch_get_main_queue(), ^{
    [[GS26ProximityShim shared] request:always != 0];
  });
}

void gs26_proximity_start(double lat, double lon, double radius_m) {
  dispatch_async(dispatch_get_main_queue(), ^{
    [[GS26ProximityShim shared] startAtLat:lat lon:lon radius:radius_m];
  });
}

void gs26_proximity_stop(void) {
  dispatch_async(dispatch_get_main_queue(), ^{
    [[GS26ProximityShim shared] stop];
  });
}

int gs26_proximity_take_wake(void) { return atomic_exchange(&g_wake, 0); }

int gs26_proximity_last_fix(double *lat, double *lon, double *accuracy_m, double *unix_s) {
  [GS26ProximityShim shared];
  int ok = 0;
  @synchronized(g_fix_lock) {
    if (!isnan(g_lat) && !isnan(g_lon) && !isnan(g_unix_s)) {
      *lat = g_lat;
      *lon = g_lon;
      *accuracy_m = g_acc;
      *unix_s = g_unix_s;
      ok = 1;
    }
  }
  return ok;
}

void gs26_proximity_open_settings(void) {
  dispatch_async(dispatch_get_main_queue(), ^{
    NSURL *url = [NSURL URLWithString:UIApplicationOpenSettingsURLString];
    if (url) {
      [[UIApplication sharedApplication] openURL:url options:@{} completionHandler:nil];
    }
  });
}
