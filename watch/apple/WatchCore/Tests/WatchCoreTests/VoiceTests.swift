import Foundation
import XCTest
@testable import WatchCore

/// The `voice` vectors in watch/test-vectors.json (the Wear OS core checks
/// the same ones).
final class VoiceTests: XCTestCase {
    static func vectors() throws -> [String: Any] {
        let packageDir = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent() // WatchCoreTests
            .deletingLastPathComponent() // Tests
            .deletingLastPathComponent() // WatchCore
        let url = packageDir.appendingPathComponent("../../test-vectors.json").standardizedFileURL
        let root = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any]
        return try XCTUnwrap(root?["voice"] as? [String: Any])
    }

    private func list(_ v: Any?) throws -> [[String: Any]] { try XCTUnwrap(v as? [[String: Any]]) }
    private func i(_ v: Any?) -> Int { (v as? NSNumber)?.intValue ?? -1 }
    private func samples(_ v: Any?) -> [Int16] { ((v as? [NSNumber]) ?? []).map { Int16($0.intValue) } }
    private func optBool(_ v: Any?) -> Bool? { v is NSNull ? nil : (v as? Bool) }
    private func hexData(_ s: String) -> Data {
        var out = Data()
        var it = s.makeIterator()
        while let a = it.next(), let b = it.next() { out.append(UInt8(String([a, b]), radix: 16)!) }
        return out
    }

    func testPtt() throws {
        let v = try Self.vectors()
        XCTAssertEqual(Int64(i(v["max_talk_ms"])), maxTalkMs)
        for c in try list(v["ptt"]) {
            let name = c["name"] as? String ?? "?"
            var machine = PttMachine(mode: PttMode(rawValue: c["mode"] as! String)!)
            for (n, step) in try list(c["steps"]).enumerated() {
                let out = machine.handle(PttEvent(rawValue: step["event"] as! String)!)
                XCTAssertEqual(out.talking, step["talking"] as? Bool, "\(name) step \(n)")
                XCTAssertEqual(out.send, optBool(step["send"]), "\(name) step \(n)")
                XCTAssertEqual(out.haptic?.rawValue, step["haptic"] as? String, "\(name) step \(n)")
            }
        }
    }

    func testBackoffAndRetry() throws {
        let b = try XCTUnwrap(Self.vectors()["backoff"] as? [String: Any])
        let base = Int64(i(b["base_ms"])), cap = Int64(i(b["cap_ms"]))
        for d in try list(b["delays"]) {
            let jitter = (d["jitter"] as! NSNumber).doubleValue
            XCTAssertEqual(reconnectDelayMs(attempt: i(d["attempt"]), jitter: jitter, baseMs: base, capMs: cap),
                           Int64(i(d["ms"])), "attempt \(i(d["attempt"]))")
        }
        for r in try list(b["retryable"]) {
            XCTAssertEqual(retryable(error: r["error"] as? String), r["retry"] as? Bool, "\(String(describing: r["error"]))")
        }
    }

    func testTransport() throws {
        for c in try list(Self.vectors()["transport"]) {
            XCTAssertEqual(voiceTransport(granted: c["granted"] as! Bool, directFailures: i(c["direct_failures"]),
                                          phoneReachable: c["phone_reachable"] as! Bool).rawValue,
                           c["transport"] as? String, c["name"] as? String ?? "?")
        }
    }

    func testFrames() throws {
        let f = try XCTUnwrap(Self.vectors()["frames"] as? [String: Any])
        for c in try list(f["downlink"]) {
            let name = c["name"] as? String ?? "?"
            let codec = VoiceCodec(rawValue: c["codec"] as! String)!
            let frame = parseDownlink(hexData(c["hex"] as! String))
            if c["speaker"] is NSNull {
                XCTAssertNil(frame, name)
                XCTAssertFalse(c["valid"] as! Bool, name)
                continue
            }
            let parsed = try XCTUnwrap(frame, name)
            XCTAssertEqual(parsed.speaker, (c["speaker"] as! NSNumber).uint32Value, name)
            XCTAssertEqual(parsed.payload.count, i(c["payload_len"]), name)
            XCTAssertEqual(payloadValid(codec: codec, length: parsed.payload.count), c["valid"] as? Bool, name)
        }
        for c in try list(f["pcm16"]) {
            let s = samples(c["samples"])
            let data = hexData(c["hex"] as! String)
            XCTAssertEqual(encodePcm16(s), data)
            XCTAssertEqual(decodePcm16(data), s)
        }
        for c in try list(f["uplink_valid"]) {
            XCTAssertEqual(payloadValid(codec: VoiceCodec(rawValue: c["codec"] as! String)!, length: i(c["len"])),
                           c["valid"] as? Bool)
        }
        let auth = try XCTUnwrap(f["auth"] as? [String: Any])
        let built = voiceAuthJSON(token: auth["token"] as! String, codec: VoiceCodec(rawValue: auth["codec"] as! String)!)
        let parsed = try JSONSerialization.jsonObject(with: Data(built.utf8)) as? NSDictionary
        XCTAssertEqual(parsed, auth["json"] as? NSDictionary)
        for c in try list(f["server"]) {
            let json = String(decoding: try JSONSerialization.data(withJSONObject: c["json"]!), as: UTF8.self)
            let message = VoiceServerMessage.parse(json)
            switch c["kind"] as? String {
            case "joined":
                XCTAssertEqual(message, .joined(id: (c["id"] as! NSNumber).uint32Value,
                                                codec: VoiceCodec(rawValue: c["codec"] as! String)!))
            case "handoff": XCTAssertEqual(message, .handoff(held: c["held"] as! Bool))
            case "floor": XCTAssertEqual(message, .floor(state: c["state"] as! String))
            case "error": XCTAssertEqual(message, .error(c["message"] as! String))
            case "radio_link": XCTAssertEqual(message, .radioLink(available: c["radio_available"] as! Bool))
            default: XCTAssertEqual(message, .unknown)
            }
        }
    }

    func testTalkers() throws {
        func roster(_ v: Any?) throws -> [RosterPeer] { try list(v).compactMap { RosterPeer($0) } }
        for c in try list(Self.vectors()["talkers"]) {
            let me = (c["self"] as! NSNumber).uint32Value
            let next = try roster(c["next"])
            XCTAssertEqual(talkers(next, selfId: me), c["talkers"] as? [String], c["name"] as? String ?? "?")
            XCTAssertEqual(startedTalking(previous: try roster(c["prev"]), next: next, selfId: me),
                           ((c["started"] as? [NSNumber]) ?? []).map(\.uint32Value))
        }
    }

    func testMixer() throws {
        let m = try XCTUnwrap(Self.vectors()["mix"] as? [String: Any])
        for c in try list(m["cases"]) {
            let name = c["name"] as? String ?? "?"
            var mixer = VoiceMixer(frameSamples: i(m["frame_samples"]), prebuffer: i(m["prebuffer"]),
                                   maxDepth: i(m["max_depth"]), volumePct: i(c["volume_pct"]))
            for (n, step) in try list(c["steps"]).enumerated() {
                if let push = step["push"] as? [Any] {
                    mixer.push((push[0] as! NSNumber).uint32Value, samples(push[1]))
                } else if let speaker = step["remove"] as? NSNumber {
                    mixer.remove(speaker.uint32Value)
                } else {
                    XCTAssertEqual(mixer.pull(), samples(step["pull"]), "\(name) step \(n)")
                }
            }
        }
    }

    func testVolume() throws {
        for c in try list(Self.vectors()["volume"]) {
            XCTAssertEqual(volumeAfterCrown(i(c["pct"]), steps: i(c["steps"])), i(c["result"]))
        }
    }

    func testVoiceSettings() throws {
        let d = WatchSettings()
        XCTAssertEqual(d.voice, VoiceSetting())
        XCTAssertEqual(d.pages.last?.id, "voice")
        let s = try JSONDecoder().decode(WatchSettings.self, from: Data(#"""
        {"voice":{"enabled":false,"channel":"crew_radio","ptt":"toggle","haptics":false,"talker_haptics":true,"route":"bluetooth","volume":999}}
        """#.utf8))
        XCTAssertFalse(s.voice.enabled)
        XCTAssertEqual(s.voice.channel, .crewRadio)
        XCTAssertEqual(s.voice.ptt, .toggle)
        XCTAssertEqual(s.voice.route, .bluetooth)
        XCTAssertTrue(s.voice.talkerHaptics)
        XCTAssertEqual(s.voice.volume, maxVoiceVolume)
        let unknown = try JSONDecoder().decode(WatchSettings.self, from: Data(#"{"voice":{"channel":"moon","ptt":"shout","route":"laser"}}"#.utf8))
        XCTAssertEqual(unknown.voice, VoiceSetting())
        // Round trip keeps snake_case keys.
        let json = String(decoding: try JSONEncoder().encode(s), as: UTF8.self)
        XCTAssertTrue(json.contains("\"talker_haptics\":true") && json.contains("\"crew_radio\""))
        XCTAssertEqual(try JSONDecoder().decode(WatchSettings.self, from: Data(json.utf8)), s)
        // Provisioning carries the voice grant.
        let p = try XCTUnwrap(DirectProvision.parse(#"{"v":1,"type":"direct","enabled":true,"addresses":["https://gs"],"token":"gsw1.x","voice":true}"#))
        XCTAssertTrue(p.voice)
        XCTAssertFalse(try XCTUnwrap(DirectProvision.parse(#"{"v":1,"type":"direct","enabled":false}"#)).voice)
    }

    func testRelayEnvelope() throws {
        let open = try JSONSerialization.jsonObject(with: Data(RelayToPhone.open(.opus).json().utf8)) as? NSDictionary
        XCTAssertEqual(open, ["v": 1, "type": "voice", "op": "open", "codec": "opus"] as NSDictionary)
        let control = try JSONSerialization.jsonObject(
            with: Data(RelayToPhone.control(voiceControlJSON("transmitting", enabled: true)).json().utf8)) as? [String: Any]
        XCTAssertEqual(control?["msg"] as? NSDictionary, ["type": "transmitting", "enabled": true] as NSDictionary)
        let frames = (0..<12).map { Data([UInt8($0)]) }
        let audio = try JSONSerialization.jsonObject(with: Data(RelayToPhone.audio(frames).json().utf8)) as? [String: Any]
        XCTAssertEqual((audio?["frames"] as? [String])?.count, 10, "at most 10 frames per message")
        let server = RelayFromPhone.parse(#"{"v":1,"type":"voice","op":"server","text":["{\"type\":\"ping\"}"],"frames":["AQIDBAU="]}"#)
        XCTAssertEqual(server, .server(text: [#"{"type":"ping"}"#], frames: [Data([1, 2, 3, 4, 5])]))
        XCTAssertEqual(RelayFromPhone.parse(#"{"v":1,"type":"voice","op":"closed","reason":"gone"}"#), .closed(reason: "gone"))
        XCTAssertNil(RelayFromPhone.parse(#"{"v":2,"type":"voice","op":"closed"}"#))
    }
}
