import Foundation
import XCTest
@testable import WatchCore

/// connection_status, auto_cycle and alerts vectors from watch/test-vectors.json
/// (the Wear OS core checks the same ones in StatusVectorsTest.kt).
final class StatusTests: XCTestCase {
    struct Vectors: Decodable {
        struct Line: Decodable { let text: String; let tone: String }
        struct Conn: Decodable {
            let name: String
            let snapshot: AnyJSON?
            let age_ms: Int64
            let reachable: Bool
            let phone: Line
            let station: Line?
            let open_phone_app: Bool
        }
        struct Step: Decodable {
            let at: Int64
            let op: String
            let to: Int?
            let index: Int
            let footer: String
        }
        struct Cycle: Decodable {
            let name: String
            let interval_s: Int
            let count: Int
            let autostart: Bool
            let steps: [Step]
        }
        struct Alerts: Decodable {
            let name: String
            let items: AnyJSON
            let expect: [[String]]
        }
        let connection_status: [Conn]
        let auto_cycle: [Cycle]
        let alerts: [Alerts]
    }

    /// Keeps a JSON sub-tree as raw bytes so it can be fed to Snapshot.parse.
    struct AnyJSON: Decodable {
        let json: String
        init(from decoder: Decoder) throws {
            let value = try decoder.singleValueContainer().decode(JSONValue.self)
            json = String(decoding: try JSONEncoder().encode(value), as: UTF8.self)
        }
    }

    indirect enum JSONValue: Codable {
        case string(String), number(Double), bool(Bool), null, array([JSONValue]), object([String: JSONValue])
        init(from decoder: Decoder) throws {
            let c = try decoder.singleValueContainer()
            if c.decodeNil() { self = .null }
            else if let b = try? c.decode(Bool.self) { self = .bool(b) }
            else if let n = try? c.decode(Double.self) { self = .number(n) }
            else if let s = try? c.decode(String.self) { self = .string(s) }
            else if let a = try? c.decode([JSONValue].self) { self = .array(a) }
            else { self = .object(try c.decode([String: JSONValue].self)) }
        }
        func encode(to encoder: Encoder) throws {
            var c = encoder.singleValueContainer()
            switch self {
            case let .string(s): try c.encode(s)
            case let .number(n): try c.encode(n)
            case let .bool(b): try c.encode(b)
            case .null: try c.encodeNil()
            case let .array(a): try c.encode(a)
            case let .object(o): try c.encode(o)
            }
        }
    }

    static func load() throws -> Vectors {
        let packageDir = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let url = packageDir.appendingPathComponent("../../test-vectors.json").standardizedFileURL
        return try JSONDecoder().decode(Vectors.self, from: Data(contentsOf: url))
    }

    func testConnectionStatusVectors() throws {
        let v = try Self.load()
        XCTAssertFalse(v.connection_status.isEmpty)
        for c in v.connection_status {
            let snap = try c.snapshot.map { try XCTUnwrap(Snapshot.parse($0.json), c.name) }
            let status = ConnectionStatus(snapshot: snap, ageMs: c.age_ms, phoneReachable: c.reachable)
            XCTAssertEqual(status.phone, StatusLine(c.phone.text, StatusTone(rawValue: c.phone.tone)!), c.name)
            XCTAssertEqual(status.station, c.station.map { StatusLine($0.text, StatusTone(rawValue: $0.tone)!) }, c.name)
            XCTAssertEqual(status.openPhoneApp, c.open_phone_app, c.name)
        }
    }

    func testAutoCycleVectors() throws {
        for c in try Self.load().auto_cycle {
            var cycle = AutoCycle(nowMs: 0, autostart: c.autostart)
            for step in c.steps {
                switch step.op {
                case "tick": cycle.tick(nowMs: step.at, intervalS: c.interval_s, count: c.count)
                case "scroll": cycle.observe(selection: step.to!, nowMs: step.at)
                case "tap": cycle.togglePause(nowMs: step.at)
                default: XCTFail("unknown op \(step.op)")
                }
                XCTAssertEqual(cycle.index, step.index, "\(c.name) @\(step.at)")
                XCTAssertEqual(cycle.footer(nowMs: step.at, intervalS: c.interval_s, count: c.count), step.footer,
                               "\(c.name) @\(step.at)")
            }
        }
    }

    func testAlertVectors() throws {
        for c in try Self.load().alerts {
            let snap = try XCTUnwrap(Snapshot.parse("{\"items\":\(c.items.json)}"), c.name)
            XCTAssertEqual(alertRows(snap).map { [$0.label, $0.text, $0.level.rawValue] }, c.expect, c.name)
        }
        XCTAssertTrue(alertRows(nil).isEmpty)
    }
}
