import Foundation

// Crew voice on the watch (docs/watch-companion.md "Voice"). Platform-neutral
// logic checked against the `voice` vectors in watch/test-vectors.json (the
// Wear OS core checks the same ones): push-to-talk state machine, reconnect
// backoff, transport choice, binary framing, PCM16, the playout mixer, the
// crown volume and the phone-relay envelope.

/// WatchConnectivity keys for the phone relay.
public let voiceWatchToPhoneKey = "gs26_watch_voice"
public let voicePhoneToWatchKey = "gs26_voice"

public enum VoiceCodec: String, Equatable {
    case pcm16, opus
}

/// 20 ms of 16 kHz mono audio.
public let voiceSampleRate = 16_000
public let voiceFrameSamples = 320
public let voicePcmFrameBytes = 640
public let maxOpusBytes = 400
/// Any talk stops after this long (stuck button guard).
public let maxTalkMs: Int64 = 60_000
/// Idle keep-alive.
public let voicePingMs: Int64 = 10_000
/// Speaker id of the recovery radio bridge.
public let radioSpeakerId: UInt32 = 0xffff_fffe

// MARK: Push-to-talk

public enum PttEvent: String {
    case ready, down, up, timeout, disconnect, held, unheld, leave
}

public enum PttHaptic: String, Equatable {
    case start, stop, denied
}

public struct PttOutput: Equatable {
    public var talking: Bool
    /// The `transmitting` control to send; nil = none.
    public var send: Bool?
    public var haptic: PttHaptic?

    public init(talking: Bool, send: Bool?, haptic: PttHaptic?) {
        self.talking = talking
        self.send = send
        self.haptic = haptic
    }
}

/// Press-and-hold or tap-to-toggle. `ready` after `joined`; a reconnect never
/// resumes talking by itself.
public struct PttMachine: Equatable {
    public var mode: PttMode
    public private(set) var ready = false
    public private(set) var held = false
    public private(set) var talking = false

    public init(mode: PttMode) { self.mode = mode }

    public mutating func handle(_ event: PttEvent) -> PttOutput {
        var send: Bool?
        var haptic: PttHaptic?
        func stop(sending: Bool) {
            if talking {
                talking = false
                haptic = .stop
                if sending { send = false }
            }
        }
        switch event {
        case .ready:
            ready = true
        case .down:
            if !ready || held {
                haptic = .denied
            } else if mode == .toggle && talking {
                stop(sending: true)
            } else if !talking {
                talking = true
                send = true
                haptic = .start
            }
        case .up:
            if mode == .hold { stop(sending: true) }
        case .timeout:
            stop(sending: true)
        case .disconnect:
            ready = false
            stop(sending: false)
        case .held:
            held = true
            stop(sending: true)
        case .unheld:
            held = false
        case .leave:
            stop(sending: true)
            ready = false
        }
        return PttOutput(talking: talking, send: send, haptic: haptic)
    }
}

// MARK: Reconnect and transport

/// `floor(min(base * 2^(attempt-1), cap) * (0.75 + 0.5 * jitter))`, attempt from 1.
public func reconnectDelayMs(attempt: Int, jitter: Double, baseMs: Int64 = 1_000, capMs: Int64 = 30_000) -> Int64 {
    let exponent = max(0, min(attempt - 1, 30))
    let raw = min(Double(baseMs) * pow(2, Double(exponent)), Double(capMs))
    return Int64((raw * (0.75 + 0.5 * jitter)).rounded(.down))
}

/// A dropped connection (no server error) or a full channel is retried; a
/// refusal (permission, sign-in, no Opus) is not.
public func retryable(error: String?) -> Bool {
    guard let error else { return true }
    return error.lowercased().contains("full")
}

public enum VoiceTransport: String, Equatable {
    case direct, phone, none
}

/// Direct to the station first; relay through the phone after two direct
/// failures in a row while the phone is reachable.
public func voiceTransport(granted: Bool, directFailures: Int, phoneReachable: Bool) -> VoiceTransport {
    guard granted else { return .none }
    if directFailures < 2 { return .direct }
    return phoneReachable ? .phone : .direct
}

// MARK: Framing

public struct DownlinkFrame: Equatable {
    public var speaker: UInt32
    public var payload: Data
}

/// Station frame: little-endian u32 speaker id then the payload. Nil when
/// there is no payload at all.
public func parseDownlink(_ data: Data) -> DownlinkFrame? {
    let bytes = [UInt8](data)
    guard bytes.count >= 5 else { return nil }
    let speaker = UInt32(bytes[0]) | UInt32(bytes[1]) << 8 | UInt32(bytes[2]) << 16 | UInt32(bytes[3]) << 24
    return DownlinkFrame(speaker: speaker, payload: Data(bytes[4...]))
}

/// Payload size valid for `codec` (both directions).
public func payloadValid(codec: VoiceCodec, length: Int) -> Bool {
    switch codec {
    case .pcm16: return length == voicePcmFrameBytes
    case .opus: return (1...maxOpusBytes).contains(length)
    }
}

public func encodePcm16(_ samples: [Int16]) -> Data {
    var out = Data(capacity: samples.count * 2)
    for s in samples {
        let u = UInt16(bitPattern: s)
        out.append(UInt8(u & 0xff))
        out.append(UInt8(u >> 8))
    }
    return out
}

public func decodePcm16(_ data: Data) -> [Int16] {
    let b = [UInt8](data)
    return stride(from: 0, to: b.count - 1, by: 2).map { Int16(bitPattern: UInt16(b[$0]) | UInt16(b[$0 + 1]) << 8) }
}

// MARK: Control messages

private func jsonString(_ object: [String: Any]) -> String {
    guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]) else { return "{}" }
    return String(decoding: data, as: UTF8.self)
}

public func voiceAuthJSON(token: String, codec: VoiceCodec) -> String {
    jsonString(["type": "auth", "token": token, "codec": codec.rawValue])
}

/// `transmitting`, `active` or `radio` with `enabled`.
public func voiceControlJSON(_ type: String, enabled: Bool) -> String {
    jsonString(["type": type, "enabled": enabled])
}

public let voicePingJSON = #"{"type":"ping"}"#

public struct RosterPeer: Equatable {
    public var id: UInt32
    public var name: String
    public var transmitting: Bool
    public var device: String

    public init(id: UInt32, name: String, transmitting: Bool, device: String) {
        self.id = id
        self.name = name
        self.transmitting = transmitting
        self.device = device
    }

    init?(_ any: Any) {
        guard let o = any as? [String: Any], let id = (o["id"] as? NSNumber)?.uint32Value else { return nil }
        self.init(id: id, name: o["name"] as? String ?? "Crew", transmitting: o["transmitting"] as? Bool ?? false,
                  device: o["device"] as? String ?? "web")
    }
}

public enum VoiceServerMessage: Equatable {
    case joined(id: UInt32, codec: VoiceCodec)
    case roster([RosterPeer])
    case handoff(held: Bool)
    /// Talk floor: `on_air`, `waiting` (the station's talker cap is full) or `idle`.
    case floor(state: String)
    case error(String)
    case radioLink(available: Bool)
    case radioState(enabled: Bool)
    case broadcastState(enabled: Bool)
    case unknown

    public static func parse(_ json: String) -> VoiceServerMessage {
        guard let data = json.data(using: .utf8),
              let o = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let type = o["type"] as? String
        else { return .unknown }
        switch type {
        case "joined":
            guard let id = (o["id"] as? NSNumber)?.uint32Value else { return .unknown }
            return .joined(id: id, codec: VoiceCodec(rawValue: o["codec"] as? String ?? "pcm16") ?? .pcm16)
        case "roster":
            return .roster(((o["peers"] as? [Any]) ?? []).compactMap { RosterPeer($0) })
        case "handoff":
            return .handoff(held: o["held"] as? Bool ?? false)
        case "floor":
            return .floor(state: o["state"] as? String ?? "idle")
        case "error":
            return .error(o["message"] as? String ?? "Voice error")
        case "radio_link":
            return .radioLink(available: (o["radio"] as? [String: Any])?["available"] as? Bool ?? false)
        case "radio_state":
            return .radioState(enabled: o["enabled"] as? Bool ?? false)
        case "broadcast_state":
            return .broadcastState(enabled: o["enabled"] as? Bool ?? false)
        default:
            return .unknown
        }
    }
}

/// Names of the others talking now, in roster order.
public func talkers(_ roster: [RosterPeer], selfId: UInt32?) -> [String] {
    roster.filter { $0.transmitting && $0.id != selfId }.map(\.name)
}

/// Ids (other than ours) that started transmitting between two rosters.
public func startedTalking(previous: [RosterPeer], next: [RosterPeer], selfId: UInt32?) -> [UInt32] {
    let before = Set(previous.filter(\.transmitting).map(\.id))
    return next.filter { $0.transmitting && $0.id != selfId && !before.contains($0.id) }.map(\.id)
}

// MARK: Playout mixer

/// Per-speaker playout buffer: waits for `prebuffer` frames before a speaker
/// plays, keeps at most `maxDepth` (oldest dropped), restarts prebuffering on
/// underrun. Mixing: integer sum × volume% / 100 (truncating toward zero),
/// clipped to Int16.
public struct VoiceMixer {
    public let frameSamples: Int
    public let prebuffer: Int
    public let maxDepth: Int
    public var volumePct: Int

    private var queues: [UInt32: [[Int16]]] = [:]
    private var playing: Set<UInt32> = []

    public init(frameSamples: Int = voiceFrameSamples, prebuffer: Int = 2, maxDepth: Int = 10, volumePct: Int = defaultVoiceVolume) {
        self.frameSamples = frameSamples
        self.prebuffer = prebuffer
        self.maxDepth = maxDepth
        self.volumePct = volumePct
    }

    public mutating func push(_ speaker: UInt32, _ samples: [Int16]) {
        guard samples.count == frameSamples else { return }
        var q = queues[speaker] ?? []
        while q.count >= maxDepth { q.removeFirst() }
        q.append(samples)
        queues[speaker] = q
    }

    public mutating func remove(_ speaker: UInt32) {
        queues[speaker] = nil
        playing.remove(speaker)
    }

    public mutating func clear() {
        queues.removeAll()
        playing.removeAll()
    }

    public mutating func pull() -> [Int16] {
        var sum = [Int](repeating: 0, count: frameSamples)
        for speaker in queues.keys.sorted() {
            var q = queues[speaker] ?? []
            if !playing.contains(speaker) && q.count >= prebuffer { playing.insert(speaker) }
            guard playing.contains(speaker) else { continue }
            if q.isEmpty {
                playing.remove(speaker)
                continue
            }
            let frame = q.removeFirst()
            queues[speaker] = q
            for i in 0..<frameSamples { sum[i] += Int(frame[i]) }
        }
        return sum.map { v in
            let scaled = v * volumePct / 100
            return Int16(clamping: scaled)
        }
    }
}

/// Crown volume: 5 % per step, 0–150 %.
public func volumeAfterCrown(_ pct: Int, steps: Int) -> Int {
    min(max(pct + 5 * steps, 0), maxVoiceVolume)
}

// MARK: Phone relay envelope

public enum RelayToPhone: Equatable {
    case open(VoiceCodec)
    /// A `transmitting`, `active`, `radio` or `ping` control.
    case control(String)
    case audio([Data])
    case close

    public func json() -> String {
        var o: [String: Any] = ["v": protocolVersion, "type": "voice"]
        switch self {
        case let .open(codec):
            o["op"] = "open"
            o["codec"] = codec.rawValue
        case let .control(json):
            o["op"] = "control"
            o["msg"] = (try? JSONSerialization.jsonObject(with: Data(json.utf8))) ?? [:]
        case let .audio(frames):
            o["op"] = "audio"
            o["frames"] = frames.prefix(10).map { $0.base64EncodedString() }
        case .close:
            o["op"] = "close"
        }
        return jsonString(o)
    }
}

public enum RelayFromPhone: Equatable {
    /// Station text messages and binary frames, in order within each list.
    case server(text: [String], frames: [Data])
    case closed(reason: String)

    public static func parse(_ json: String) -> RelayFromPhone? {
        guard let data = json.data(using: .utf8),
              let o = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              (o["v"] as? NSNumber)?.intValue == protocolVersion, o["type"] as? String == "voice"
        else { return nil }
        switch o["op"] as? String {
        case "server":
            let text = (o["text"] as? [String]) ?? []
            let frames = ((o["frames"] as? [String]) ?? []).compactMap { Data(base64Encoded: $0) }
            return .server(text: text, frames: frames)
        case "closed":
            return .closed(reason: o["reason"] as? String ?? "")
        default:
            return nil
        }
    }
}
