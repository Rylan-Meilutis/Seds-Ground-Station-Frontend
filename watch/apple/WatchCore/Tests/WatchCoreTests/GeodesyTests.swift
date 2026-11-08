import Foundation
import XCTest
@testable import WatchCore

/// Same checks as `shared_test_vectors_match` in geodesy.rs.
final class GeodesyTests: XCTestCase {
    struct Vectors: Decodable {
        struct Geo: Decodable {
            let name: String
            let from: [Double]
            let to: [Double]
            let distance_m: Double
            let distance_tol_m: Double
            let bearing_deg: Bearing
        }
        enum Bearing: Decodable {
            case value(Double), null, any
            init(from decoder: Decoder) throws {
                let c = try decoder.singleValueContainer()
                if c.decodeNil() { self = .null; return }
                if let d = try? c.decode(Double.self) { self = .value(d); return }
                guard try c.decode(String.self) == "any" else {
                    throw DecodingError.dataCorruptedError(in: c, debugDescription: "bad bearing")
                }
                self = .any
            }
        }
        struct Dist: Decodable { let m: Double; let metric: String; let imperial: String }
        struct Age: Decodable { let ms: Int64; let text: String }
        struct Arrow: Decodable {
            let bearing_true: Double
            let heading_true: Double
            let declination: Double
            let arrow: Double
            let bearing_magnetic: Double
        }
        let geodesy: [Geo]
        let distance_format: [Dist]
        let age_format: [Age]
        let arrow: [Arrow]
    }

    static func loadVectors() throws -> Vectors {
        let packageDir = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent() // WatchCoreTests
            .deletingLastPathComponent() // Tests
            .deletingLastPathComponent() // WatchCore
        let url = packageDir.appendingPathComponent("../../test-vectors.json").standardizedFileURL
        return try JSONDecoder().decode(Vectors.self, from: Data(contentsOf: url))
    }

    func testSharedVectors() throws {
        let v = try Self.loadVectors()
        XCTAssertFalse(v.geodesy.isEmpty)
        for c in v.geodesy {
            let a = LatLon(lat: c.from[0], lon: c.from[1])
            let b = LatLon(lat: c.to[0], lon: c.to[1])
            let d = try XCTUnwrap(distanceM(a, b), c.name)
            XCTAssertEqual(d, c.distance_m, accuracy: c.distance_tol_m, c.name)
            switch c.bearing_deg {
            case .any:
                continue
            case let .value(want):
                let got = try XCTUnwrap(initialBearingDeg(a, b), c.name)
                XCTAssertLessThanOrEqual(abs(signedDeltaDeg(from: got, to: want)), 0.01, "\(c.name): \(got) vs \(want)")
            case .null:
                XCTAssertNil(initialBearingDeg(a, b), c.name)
            }
        }
        for c in v.distance_format {
            XCTAssertEqual(formatDistance(c.m, metric: true), c.metric, "\(c.m)")
            XCTAssertEqual(formatDistance(c.m, metric: false), c.imperial, "\(c.m)")
        }
        for c in v.age_format {
            XCTAssertEqual(formatAge(c.ms), c.text)
        }
        for c in v.arrow {
            XCTAssertEqual(arrowRotationDeg(c.bearing_true, c.heading_true), c.arrow, accuracy: 1e-9)
            XCTAssertEqual(trueToMagneticDeg(c.bearing_true, c.declination), c.bearing_magnetic, accuracy: 1e-9)
        }
    }

    func testShortDistances() throws {
        let a = LatLon(lat: 32.9904, lon: -106.9750)
        let b = LatLon(lat: 32.9905, lon: -106.9750)
        XCTAssertEqual(try XCTUnwrap(distanceM(a, b)), 11.1195, accuracy: 0.001)
        XCTAssertEqual(try XCTUnwrap(initialBearingDeg(a, b)), 0, accuracy: 1e-6)
        let east = LatLon(lat: 32.9904, lon: -106.9750 + 10.0 / (111_195.08 * cos(32.9904 * Double.pi / 180)))
        XCTAssertEqual(try XCTUnwrap(distanceM(a, east)), 10, accuracy: 0.01)
        XCTAssertEqual(try XCTUnwrap(initialBearingDeg(a, east)), 90, accuracy: 0.01)
        let sub = LatLon(lat: 40.000005, lon: -75)
        XCTAssertNil(initialBearingDeg(LatLon(lat: 40, lon: -75), sub))
        XCTAssertEqual(distanceM(sub, sub), 0)
        XCTAssertNil(initialBearingDeg(sub, sub))
    }

    func testNearPole() throws {
        let a = LatLon(lat: 89.9, lon: 0)
        let b = LatLon(lat: 89.9, lon: 180)
        XCTAssertEqual(try XCTUnwrap(distanceM(a, b)), 0.2 * Double.pi / 180 * earthRadiusM, accuracy: 0.01)
        XCTAssertEqual(try XCTUnwrap(initialBearingDeg(a, b)), 0, accuracy: 1e-6)
        XCTAssertEqual(try XCTUnwrap(initialBearingDeg(b, a)), 0, accuracy: 1e-6)
        XCTAssertEqual(try XCTUnwrap(initialBearingDeg(a, LatLon(lat: 89.9, lon: 90))), 45, accuracy: 0.01)
        XCTAssertNil(initialBearingDeg(LatLon(lat: 90, lon: 0), a))
        XCTAssertNotNil(distanceM(LatLon(lat: 90, lon: 0), a))
        let s = try XCTUnwrap(initialBearingDeg(LatLon(lat: -89.95, lon: 10), LatLon(lat: -89.95, lon: -170)))
        XCTAssertEqual(abs(signedDeltaDeg(from: s, to: 180)), 0, accuracy: 1e-6)
    }

    func testInvalidInputs() {
        XCTAssertNil(distanceM(LatLon(lat: .nan, lon: 0), LatLon(lat: 0, lon: 0)))
        XCTAssertNil(distanceM(LatLon(lat: 91, lon: 0), LatLon(lat: 0, lon: 0)))
        XCTAssertNil(initialBearingDeg(LatLon(lat: 0, lon: 181), LatLon(lat: 0, lon: 0)))
    }

    func testAngles() {
        XCTAssertEqual(trueToMagneticDeg(0, 10), 350, accuracy: 1e-9)
        XCTAssertEqual(magneticToTrueDeg(350, 10), 0, accuracy: 1e-9)
        XCTAssertEqual(trueToMagneticDeg(90, -7.5), 97.5, accuracy: 1e-9)
        XCTAssertEqual(signedDeltaDeg(from: 350, to: 10), 20, accuracy: 1e-9)
        XCTAssertEqual(signedDeltaDeg(from: 10, to: 350), -20, accuracy: 1e-9)
        XCTAssertEqual(normalizeDeg(-1e-15), 0)
        XCTAssertEqual(normalizeDeg(720), 0)
        XCTAssertEqual(normalizeDeg(-90), 270)
        XCTAssertEqual(cardinal(359), "N")
        XCTAssertEqual(cardinal(200), "S")
        XCTAssertEqual(cardinal(300), "NW")
    }

    func testFormatting() {
        XCTAssertEqual(formatDistance(100, metric: false), "328 ft")
        XCTAssertEqual(formatDistance(.nan, metric: true), "--")
        XCTAssertEqual(formatDistance(-1, metric: true), "--")
        XCTAssertEqual(formatAge(4_900), "4s")
        XCTAssertEqual(formatAge(125_000), "2m 05s")
        XCTAssertEqual(formatAge(3_780_000), "1h 03m")
    }
}
