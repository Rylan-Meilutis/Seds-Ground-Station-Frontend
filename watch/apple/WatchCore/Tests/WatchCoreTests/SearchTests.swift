import XCTest
@testable import WatchCore

final class SearchTests: XCTestCase {
    private let on = SearchInfo(active: true, sessionId: "rs-1", sharing: true, ackedSeq: 0)

    func testNothingIsRecordedWithoutTheUsersOptInAndTheStationsAcceptance() {
        var r = SearchRecorder()
        XCTAssertFalse(r.record(lat: 31, lon: -102, accuracyM: 5, tMs: 0, info: on), "the user did not opt in")
        r.wantsToShare = true
        XCTAssertFalse(r.record(lat: 31, lon: -102, accuracyM: 5, tMs: 0, info: nil), "no search data")
        let notAccepted = SearchInfo(active: true, sessionId: "rs-1", sharing: false, ackedSeq: 0)
        XCTAssertFalse(r.record(lat: 31, lon: -102, accuracyM: 5, tMs: 0, info: notAccepted))
        let stopped = SearchInfo(active: false, sessionId: "rs-1", sharing: true, ackedSeq: 0)
        XCTAssertFalse(r.record(lat: 31, lon: -102, accuracyM: 5, tMs: 0, info: stopped))
        r.apply(on)
        XCTAssertTrue(r.record(lat: 31, lon: -102, accuracyM: 5, tMs: 0, info: on))
        r.stop()
        XCTAssertTrue(r.pending.isEmpty)
        XCTAssertFalse(r.record(lat: 31.001, lon: -102, accuracyM: 5, tMs: 60_000, info: on))
    }

    func testThinningBufferingAndAcknowledgement() throws {
        var r = SearchRecorder()
        r.wantsToShare = true
        r.apply(on)
        XCTAssertTrue(r.record(lat: 31, lon: -102, accuracyM: 5, tMs: 0, info: on))
        XCTAssertFalse(r.record(lat: 31.00001, lon: -102, accuracyM: 5, tMs: 1_000, info: on), "1 m")
        XCTAssertTrue(r.record(lat: 31.0001, lon: -102, accuracyM: 5, tMs: 2_000, info: on), "11 m")
        XCTAssertTrue(r.record(lat: 31.0001, lon: -102, accuracyM: nil, tMs: 40_000, info: on), "30 s")
        XCTAssertFalse(r.record(lat: 31.01, lon: -102, accuracyM: 250, tMs: 50_000, info: on), "poor accuracy")
        let message = try XCTUnwrap(r.nextMessage())
        let json = try XCTUnwrap(message.json())
        XCTAssertTrue(json.contains("\"type\":\"search_track\""))
        XCTAssertTrue(json.contains("\"session_id\":\"rs-1\""))
        XCTAssertEqual(r.pending.map(\.seq), [1, 2, 3])
        // The phone holds 1 and 2; 3 is resent.
        r.apply(SearchInfo(active: true, sessionId: "rs-1", sharing: true, ackedSeq: 2))
        XCTAssertEqual(r.pending.map(\.seq), [3])
        // A new session drops the buffer and the opt-in.
        r.apply(SearchInfo(active: true, sessionId: "rs-2", sharing: false, ackedSeq: 0))
        XCTAssertTrue(r.pending.isEmpty)
        XCTAssertFalse(r.wantsToShare)
    }

    func testTheSnapshotSearchObjectDecodes() throws {
        let snap = try XCTUnwrap(Snapshot.parse(#"{"v":1,"seq":1,"mode":"live","search":{"active":true,"session_id":"rs-1","sharing":true,"acked_seq":7,"percent":12.5,"swath_m":30}}"#))
        XCTAssertEqual(snap.search, SearchInfo(active: true, sessionId: "rs-1", sharing: true, ackedSeq: 7, percent: 12.5, swathM: 30))
        XCTAssertNil(Snapshot.parse(#"{"v":1,"seq":1,"mode":"live"}"#)?.search)
        XCTAssertEqual(WatchMessage.searchShare(true).json(), #"{"sharing":true,"type":"search_share","v":1}"#)
    }
}
