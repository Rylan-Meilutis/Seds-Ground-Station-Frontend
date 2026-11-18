import Foundation
import XCTest
@testable import WatchCore

/// link_route, action_outcome, action_button, page_layout, provision and
/// direct_status vectors from watch/test-vectors.json (the Wear OS core
/// checks the same ones in DirectVectorsTest.kt).
final class DirectTests: XCTestCase {
    struct PhoneIn: Decodable { let mode: String; let age_ms: Int64 }
    struct Route: Decodable {
        let name: String
        let phone: PhoneIn?
        let direct_age_ms: Int64?
        let fresh_ms: Int64
        let foreground: Bool
        let configured: Bool
        let route: String
        let poll: Bool
    }
    struct Outcome: Decodable {
        let name: String
        let status: Int?
        let body: String
        let error: String?
        let kind: String
        let text: String
    }
    struct ButtonExpect: Decodable { let enabled: Bool; let note: String?; let confirm: String }
    struct Button: Decodable {
        let name: String
        let abort: Bool
        let enabled: Bool
        let reason: String?
        let fresh: Bool
        let token: Bool
        let expect: ButtonExpect
    }
    struct LayoutPage: Decodable { let id: String; let kind: String; let items: [String]; let actions: [String] }
    struct Layout: Decodable {
        let name: String
        let settings: StatusTests.AnyJSON
        let catalog_pages: StatusTests.AnyJSON
        let published: StatusTests.AnyJSON
        let expect: [LayoutPage]
    }
    struct ProvisionExpect: Decodable {
        let configured: Bool
        let base: String
        let address: String
        let token: String?
        let commands: [String]
    }
    struct ProvisionCase: Decodable { let name: String; let json: StatusTests.AnyJSON; let expect: ProvisionExpect? }
    struct MergeExpect: Decodable {
        let mode: String
        let source: String
        let station_address: String
        let station_link: String
        let page_ids: [String]
        let altitude: String
        let targets: Int
        let dump_enabled: Bool
        let dump_reason: String
        let units: String
    }
    struct Merge: Decodable {
        let base: StatusTests.AnyJSON
        let direct: StatusTests.AnyJSON
        let address: String
        let expect: MergeExpect
    }
    struct DirectStatus: Decodable { let item_ids: [String]; let merge: Merge }
    struct Vectors: Decodable {
        let link_route: [Route]
        let action_outcome: [Outcome]
        let action_button: [Button]
        let page_layout: [Layout]
        let provision: [ProvisionCase]
        let direct_status: DirectStatus
    }

    static func load() throws -> Vectors {
        let packageDir = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let url = packageDir.appendingPathComponent("../../test-vectors.json").standardizedFileURL
        return try JSONDecoder().decode(Vectors.self, from: Data(contentsOf: url))
    }

    func testLinkRouteVectors() throws {
        let v = try Self.load()
        XCTAssertGreaterThanOrEqual(v.link_route.count, 10)
        for c in v.link_route {
            let mode = c.phone.map { Mode(rawValue: $0.mode)! }
            let route = chooseRoute(phoneMode: mode, phoneAgeMs: c.phone?.age_ms, directAgeMs: c.direct_age_ms, freshMs: c.fresh_ms)
            XCTAssertEqual(route.rawValue, c.route, c.name)
            let poll = shouldPollDirect(phoneMode: mode, phoneAgeMs: c.phone?.age_ms, freshMs: c.fresh_ms,
                                        foreground: c.foreground, configured: c.configured)
            XCTAssertEqual(poll, c.poll, c.name)
        }
        XCTAssertEqual(phoneFreshMs(.balanced), 6_000)
        XCTAssertEqual(phoneFreshMs(.saver), 15_000)
        XCTAssertEqual(directPollMs(.recovery), 2_000)
    }

    func testActionOutcomeVectors() throws {
        for c in try Self.load().action_outcome {
            let r = actionOutcome(status: c.status, body: c.body, error: c.error)
            XCTAssertEqual(r.outcome.rawValue, c.kind, c.name)
            XCTAssertEqual(r.text, c.text, c.name)
        }
    }

    func testActionButtonVectors() throws {
        for c in try Self.load().action_button {
            let b = actionButton(abort: c.abort, state: ActionState(enabled: c.enabled, reason: c.reason),
                                 fresh: c.fresh, token: c.token)
            XCTAssertEqual(b.enabled, c.expect.enabled, c.name)
            XCTAssertEqual(b.note, c.expect.note, c.name)
            XCTAssertEqual(b.confirm.rawValue, c.expect.confirm, c.name)
        }
    }

    func testPageLayoutVectors() throws {
        let v = try Self.load()
        XCTAssertGreaterThanOrEqual(v.page_layout.count, 4)
        for c in v.page_layout {
            let settings = try JSONDecoder().decode(WatchSettings.self, from: Data(c.settings.json.utf8))
            let catalog = try JSONDecoder().decode([Page].self, from: Data(c.catalog_pages.json.utf8))
            let published = try JSONDecoder().decode([Page].self, from: Data(c.published.json.utf8))
            let pages = resolvedPages(settings, catalog: catalog, published: published)
            XCTAssertEqual(pages.map(\.id), c.expect.map(\.id), c.name)
            for (page, want) in zip(pages, c.expect) {
                XCTAssertEqual(page.kind, want.kind, "\(c.name) \(page.id)")
                XCTAssertEqual(page.items, want.items, "\(c.name) \(page.id)")
                XCTAssertEqual(page.actions.map(\.cmd), want.actions, "\(c.name) \(page.id)")
                XCTAssertLessThanOrEqual(page.actions.count, maxPageActions)
            }
        }
    }

    func testProvisionVectors() throws {
        for c in try Self.load().provision {
            let p = DirectProvision.parse(c.json.json)
            guard let want = c.expect else {
                XCTAssertNil(p, c.name)
                continue
            }
            let got = try XCTUnwrap(p, c.name)
            XCTAssertEqual(got.configured, want.configured, c.name)
            XCTAssertEqual(got.base, want.base, c.name)
            XCTAssertEqual(got.address, want.address, c.name)
            XCTAssertEqual(got.token, want.token, c.name)
            XCTAssertEqual(got.commands, want.commands, c.name)
        }
    }

    func testDirectStatusMerge() throws {
        let m = try Self.load().direct_status.merge
        let base = try XCTUnwrap(Snapshot.parse(m.base.json))
        let direct = try XCTUnwrap(Snapshot.parse(m.direct.json))
        let merged = direct.mergedDirect(base: base, address: m.address)
        XCTAssertEqual(merged.mode.rawValue, m.expect.mode)
        XCTAssertEqual(merged.source, m.expect.source)
        XCTAssertEqual(merged.station?.address, m.expect.station_address)
        XCTAssertEqual(merged.station?.link, m.expect.station_link)
        XCTAssertEqual(merged.pages.map(\.id), m.expect.page_ids)
        XCTAssertEqual(merged.items["altitude"]?.text, m.expect.altitude)
        XCTAssertEqual(merged.targets.count, m.expect.targets)
        XCTAssertEqual(merged.actions["Dump"]?.enabled, m.expect.dump_enabled)
        XCTAssertEqual(merged.actions["Dump"]?.reason, m.expect.dump_reason)
        XCTAssertEqual(merged.settings?.units.rawValue, m.expect.units)
        XCTAssertEqual(pageActionCommands(merged.pages), ["Abort", "Dump"])
    }

    func testRequests() throws {
        let url = try XCTUnwrap(statusURL(base: "https://gs.local:3000", units: .imperial, actions: ["Abort", "Dump"], staleAfterS: 30))
        XCTAssertEqual(url.absoluteString,
                       "https://gs.local:3000/api/watch/status?units=imperial&stale_after_s=30&actions=Abort,Dump")
        XCTAssertEqual(commandURL(base: "http://192.168.3.7:3000")?.absoluteString, "http://192.168.3.7:3000/api/command")
        XCTAssertEqual(String(decoding: commandBody("Abort"), as: UTF8.self), "\"Abort\"")
    }
}
