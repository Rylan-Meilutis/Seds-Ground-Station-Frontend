import CoreLocation
import Foundation
import WatchCore

/// Watch position and heading, running only while the compass page is
/// visible and the app is active.
final class LocationService: NSObject, ObservableObject, CLLocationManagerDelegate {
    @Published private(set) var location: CLLocation? = nil
    /// Degrees from true north (see `didUpdateHeading`).
    @Published private(set) var trueHeading: Double? = nil
    @Published private(set) var declination: Double? = nil
    @Published private(set) var needsCalibration = false
    @Published private(set) var authorization: CLAuthorizationStatus

    let headingAvailable = CLLocationManager.headingAvailable()

    // Created on the main thread, so delegate callbacks arrive on main.
    private let manager: CLLocationManager
    private var running = false

    override init() {
        let manager = CLLocationManager()
        self.manager = manager
        _authorization = Published(initialValue: manager.authorizationStatus)
        super.init()
        manager.delegate = self
        manager.headingFilter = 2
    }

    var reading: CompassReading {
        CompassReading(
            position: location.map { LatLon(lat: $0.coordinate.latitude, lon: $0.coordinate.longitude) },
            trueHeading: trueHeading,
            declination: declination
        )
    }

    var denied: Bool { authorization == .denied || authorization == .restricted }

    func start(recovery: Bool) {
        manager.desiredAccuracy = recovery ? kCLLocationAccuracyBest : kCLLocationAccuracyNearestTenMeters
        manager.distanceFilter = recovery ? 2 : 5
        if manager.authorizationStatus == .notDetermined {
            manager.requestWhenInUseAuthorization()
        }
        guard !running else { return }
        running = true
        if let last = location, -last.timestamp.timeIntervalSinceNow > 120 {
            location = nil // too old to aim with; show "Waiting for watch GPS"
        }
        manager.startUpdatingLocation()
        if headingAvailable { manager.startUpdatingHeading() }
    }

    func stop() {
        guard running else { return }
        running = false
        manager.stopUpdatingLocation()
        if headingAvailable { manager.stopUpdatingHeading() }
    }

    // MARK: CLLocationManagerDelegate

    func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard let last = locations.last, last.horizontalAccuracy >= 0 else { return }
        location = last
    }

    func locationManager(_ manager: CLLocationManager, didUpdateHeading newHeading: CLHeading) {
        let accuracy = newHeading.headingAccuracy
        needsCalibration = accuracy < 0 || accuracy > 25
        guard accuracy >= 0 else {
            trueHeading = nil
            return
        }
        let magnetic = newHeading.magneticHeading
        if newHeading.trueHeading >= 0 {
            trueHeading = newHeading.trueHeading
            if magnetic >= 0 { declination = signedDeltaDeg(from: magnetic, to: newHeading.trueHeading) }
        } else {
            // No true heading (e.g. no location yet): magnetic heading,
            // corrected with the last known declination when we have one.
            trueHeading = declination.map { magneticToTrueDeg(magnetic, $0) } ?? magnetic
        }
    }

    func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        // Transient (kCLErrorLocationUnknown) or heading interference; keep
        // the last values and wait for the next update.
    }

    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        authorization = manager.authorizationStatus
    }
}
