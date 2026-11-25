import AVFoundation
import Combine
import Foundation
import WatchCore
import WatchKit
#if canImport(CallKit)
import CallKit
#endif

/// Crew voice on the watch (docs/watch-companion.md "Voice").
///
/// * Background: the session runs as a CallKit call ("GS crew voice"). On
///   watchOS a CallKit call is what lets an app keep recording, playing and
///   using a socket (low-level networking, TN3135) with the wrist down.
/// * Transport: `URLSessionWebSocketTask` straight to the station's
///   `/api/voice/ws` with the watch token, or the same protocol relayed by
///   the phone (WatchCore `voiceTransport`).
/// * Codec: Opus (16 kHz mono, one packet per 20 ms) through
///   `AVAudioConverter`; PCM16 when this watch cannot create the Opus
///   converter.
///
/// UNVERIFIED: written on Linux without a Swift toolchain. Points to check on
/// a device are marked "Device check".
@MainActor
final class VoiceSession: NSObject, ObservableObject {
    enum Phase: Equatable {
        case idle
        case permission
        case connecting
        case connected
        case reconnecting(inMs: Int64)
        case failed(String)
    }

    @Published private(set) var phase = Phase.idle
    @Published private(set) var transport = VoiceTransport.none
    @Published private(set) var talking = false
    /// Paused because another watch of ours holds the floor.
    @Published private(set) var held = false
    /// Talking, but the station's talker cap is full: queued, not on air.
    @Published private(set) var waiting = false
    @Published private(set) var talkers: [String] = []
    @Published private(set) var peerCount = 0
    @Published private(set) var radioAvailable = false
    @Published private(set) var codec = VoiceCodec.opus
    @Published private(set) var micDenied = false
    /// Playback volume shown while the crown turns (saved debounced).
    @Published var volumePct = defaultVoiceVolume {
        didSet { playback.setVolume(volumePct) }
    }

    private weak var link: PhoneLink?
    private var ptt = PttMachine(mode: .hold)
    private var selfId: UInt32?
    private var roster: [RosterPeer] = []
    private var lastServerError: String?
    private var everJoined = false
    private var userLeft = true
    private var attempts = 0
    private var directFailures = 0
    private var pingTimer: Timer?
    private var talkTimer: Timer?
    private var reconnectTimer: Timer?
    private var saveTimer: Timer?

    // Socket
    private let trust = TrustDelegate()
    private lazy var urlSession = URLSession(configuration: .ephemeral, delegate: trust, delegateQueue: nil)
    private var socket: URLSessionWebSocketTask?
    private var relayFrames: [Data] = []

    // Audio
    private let engine = AVAudioEngine()
    private let playback = PlaybackMixer()
    private let capture = CaptureGate()
    private var sourceNode: AVAudioSourceNode?
    private var decoders: [UInt32: OpusDecoder] = [:]
    private var engineRunning = false

    #if canImport(CallKit)
    private var provider: CXProvider?
    private let callController = CXCallController()
    private var callId: UUID?
    #endif

    init(link: PhoneLink) {
        self.link = link
        super.init()
        link.voiceRelayHandler = { [weak self] json in self?.relayReceived(json) }
        volumePct = link.settings?.voice.volume ?? defaultVoiceVolume
    }

    private var settings: VoiceSetting { link?.settings?.voice ?? VoiceSetting() }
    private var provision: DirectProvision? { link?.direct.provision }

    var joined: Bool { !userLeft }
    var granted: Bool { provision?.voice == true && provision?.configured == true }
    var channelName: String { settings.channel == .crewRadio ? "Crew + radio" : "Crew" }

    // MARK: Join / leave

    /// Asks for the microphone if needed, then starts the call and connects.
    func join() {
        guard userLeft else { return }
        guard granted else {
            phase = .failed(provision?.configured == true
                ? "Voice is not enabled for this account. Ask an admin for voice permission."
                : "Set up the direct link on the phone (Settings → Watch).")
            return
        }
        switch AVAudioApplication.shared.recordPermission {
        case .granted:
            start()
        case .denied:
            micDenied = true
            phase = .failed("Microphone access is off. Allow it in Settings → Privacy & Security → Microphone → GS26.")
        default:
            phase = .permission
            AVAudioApplication.requestRecordPermission { [weak self] ok in
                Task { @MainActor in
                    guard let self else { return }
                    self.micDenied = !ok
                    if ok {
                        self.start()
                    } else {
                        self.phase = .failed("Microphone access is off. Allow it in Settings → Privacy & Security → Microphone → GS26.")
                    }
                }
            }
        }
    }

    func leave() {
        guard !userLeft else { return }
        apply(ptt.handle(.leave))
        userLeft = true
        if transport == .phone { link?.sendVoiceRelay(RelayToPhone.close.json()) }
        closeSocket()
        stopTimers()
        endCall()
        stopAudio()
        decoders.removeAll()
        playback.clear()
        roster = []
        talkers = []
        peerCount = 0
        selfId = nil
        phase = .idle
        transport = .none
    }

    private func start() {
        userLeft = false
        attempts = 0
        everJoined = false
        micDenied = false
        ptt = PttMachine(mode: settings.ptt)
        volumePct = settings.volume
        phase = .connecting
        startCall()
    }

    // MARK: Push-to-talk

    func pressDown() {
        if held == false, transport != .none, !talking {
            // A yielded watch claims the floor again before talking.
            sendText(voiceControlJSON("active", enabled: true))
        }
        apply(ptt.handle(.down))
    }

    func pressUp() { apply(ptt.handle(.up)) }

    private func apply(_ out: PttOutput) {
        if let send = out.send { sendText(voiceControlJSON("transmitting", enabled: send)) }
        if let haptic = out.haptic, settings.haptics {
            switch haptic {
            case .start: WKInterfaceDevice.current().play(.start)
            case .stop: WKInterfaceDevice.current().play(.stop)
            case .denied: WKInterfaceDevice.current().play(.failure)
            }
        }
        if out.talking != talking {
            talking = out.talking
            capture.set(out.talking)
            talkTimer?.invalidate()
            talkTimer = nil
            if out.talking {
                talkTimer = Timer.scheduledTimer(withTimeInterval: Double(maxTalkMs) / 1000, repeats: false) { [weak self] _ in
                    Task { @MainActor in self.map { $0.apply($0.ptt.handle(.timeout)) } }
                }
            }
        }
    }

    // MARK: Volume (Digital Crown)

    func crownVolume(_ pct: Int) {
        let clamped = min(max(pct, 0), maxVoiceVolume)
        guard clamped != volumePct else { return }
        volumePct = clamped
        saveTimer?.invalidate()
        saveTimer = Timer.scheduledTimer(withTimeInterval: 1.5, repeats: false) { [weak self] _ in
            Task { @MainActor in
                guard let self else { return }
                let pct = self.volumePct
                self.link?.edit { $0.voice.volume = pct }
            }
        }
    }

    // MARK: Connection

    private func connect() {
        guard !userLeft else { return }
        lastServerError = nil
        let chosen = voiceTransport(granted: granted, directFailures: directFailures,
                                    phoneReachable: link?.phoneReachable ?? false)
        transport = chosen
        codec = OpusEncoder.available ? .opus : .pcm16
        phase = attempts == 0 ? .connecting : phase
        switch chosen {
        case .none:
            fail("Voice is not enabled for this account.")
        case .direct:
            guard let provision, let token = provision.token, let url = voiceSocketURL(provision.base) else {
                fail("Set up the direct link on the phone.")
                return
            }
            trust.insecureHost = provision.skipTLS ? URL(string: provision.base)?.host : nil
            let task = urlSession.webSocketTask(with: url)
            task.maximumMessageSize = 64 * 1024
            socket = task
            task.resume()
            sendText(voiceAuthJSON(token: token, codec: codec))
            receive(on: task)
        case .phone:
            link?.sendVoiceRelay(RelayToPhone.open(codec).json())
        }
        pingTimer?.invalidate()
        pingTimer = Timer.scheduledTimer(withTimeInterval: Double(voicePingMs) / 1000, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.sendText(voicePingJSON) }
        }
    }

    private func voiceSocketURL(_ base: String) -> URL? {
        guard var c = URLComponents(string: base) else { return nil }
        c.scheme = c.scheme == "https" ? "wss" : "ws"
        c.path = (c.path.hasSuffix("/") ? String(c.path.dropLast()) : c.path) + "/api/voice/ws"
        return c.url
    }

    private func receive(on task: URLSessionWebSocketTask) {
        task.receive { [weak self] result in
            Task { @MainActor in
                guard let self, self.socket === task else { return }
                switch result {
                case let .success(.string(text)):
                    self.serverText(text)
                    self.receive(on: task)
                case let .success(.data(data)):
                    self.serverFrame(data)
                    self.receive(on: task)
                case .success:
                    self.receive(on: task)
                case .failure:
                    self.connectionLost()
                }
            }
        }
    }

    private func sendText(_ json: String) {
        switch transport {
        case .direct:
            socket?.send(.string(json)) { _ in }
        case .phone:
            if json.contains(#""type":"auth""#) { return } // the phone authenticates itself
            link?.sendVoiceRelay(RelayToPhone.control(json).json())
        case .none:
            break
        }
    }

    /// One encoded 20 ms frame from the microphone.
    fileprivate func sendFrame(_ frame: Data) {
        guard talking, payloadValid(codec: codec, length: frame.count) else { return }
        switch transport {
        case .direct:
            socket?.send(.data(frame)) { _ in }
        case .phone:
            // 100 ms per relay message keeps WatchConnectivity traffic low.
            relayFrames.append(frame)
            if relayFrames.count >= 5 { flushRelay() }
        case .none:
            break
        }
    }

    private func flushRelay() {
        guard !relayFrames.isEmpty else { return }
        link?.sendVoiceRelay(RelayToPhone.audio(relayFrames).json())
        relayFrames.removeAll()
    }

    private func relayReceived(_ json: String) {
        guard transport == .phone, !userLeft, let message = RelayFromPhone.parse(json) else { return }
        switch message {
        case let .server(text, frames):
            text.forEach(serverText)
            frames.forEach(serverFrame)
        case let .closed(reason):
            if !reason.isEmpty, lastServerError == nil { lastServerError = reason }
            connectionLost()
        }
    }

    private func serverText(_ json: String) {
        switch VoiceServerMessage.parse(json) {
        case let .joined(id, joinedCodec):
            selfId = id
            codec = joinedCodec
            everJoined = true
            attempts = 0
            if transport == .direct { directFailures = 0 }
            phase = .connected
            apply(ptt.handle(.ready))
            reportConnected()
        case let .roster(peers):
            let started = startedTalking(previous: roster, next: peers, selfId: selfId)
            roster = peers
            peerCount = peers.count
            talkers = WatchCore.talkers(peers, selfId: selfId)
            for gone in Set(decoders.keys).subtracting(peers.map(\.id)) where gone != radioSpeakerId {
                decoders[gone] = nil
                playback.remove(gone)
            }
            if !started.isEmpty, settings.talkerHaptics { WKInterfaceDevice.current().play(.notification) }
        case let .handoff(isHeld):
            held = isHeld
            apply(ptt.handle(isHeld ? .held : .unheld))
        case let .floor(state):
            waiting = state == "waiting"
        case let .error(message):
            lastServerError = message
        case let .radioLink(available):
            let changed = available != radioAvailable
            radioAvailable = available
            if changed, settings.channel == .crewRadio { sendText(voiceControlJSON("radio", enabled: available)) }
        case .radioState, .broadcastState, .unknown:
            break
        }
    }

    private func serverFrame(_ data: Data) {
        guard let frame = parseDownlink(data), payloadValid(codec: codec, length: frame.payload.count) else { return }
        let samples: [Int16]?
        switch codec {
        case .pcm16:
            samples = decodePcm16(frame.payload)
        case .opus:
            if decoders[frame.speaker] == nil { decoders[frame.speaker] = OpusDecoder() }
            samples = decoders[frame.speaker]?.decode(frame.payload)
        }
        if let samples { playback.push(frame.speaker, samples) }
        if frame.speaker == radioSpeakerId, !talkers.contains("Recovery radio") {
            talkers.append("Recovery radio")
        }
    }

    private func connectionLost() {
        guard socket != nil || transport == .phone else { return }
        closeSocket()
        apply(ptt.handle(.disconnect))
        held = false
        pingTimer?.invalidate()
        guard !userLeft else { return }
        if !everJoined, transport == .direct { directFailures += 1 }
        everJoined = false
        guard retryable(error: lastServerError) else {
            fail(lastServerError ?? "Voice refused")
            return
        }
        attempts += 1
        let delay = reconnectDelayMs(attempt: attempts, jitter: Double.random(in: 0..<1))
        phase = .reconnecting(inMs: delay)
        reconnectTimer?.invalidate()
        reconnectTimer = Timer.scheduledTimer(withTimeInterval: Double(delay) / 1000, repeats: false) { [weak self] _ in
            Task { @MainActor in self?.connect() }
        }
    }

    private func fail(_ message: String) {
        leave()
        phase = .failed(message)
    }

    private func closeSocket() {
        socket?.cancel(with: .goingAway, reason: nil)
        socket = nil
        relayFrames.removeAll()
    }

    private func stopTimers() {
        [pingTimer, talkTimer, reconnectTimer].forEach { $0?.invalidate() }
        pingTimer = nil
        talkTimer = nil
        reconnectTimer = nil
    }

    // MARK: Audio

    private func configureSession() throws {
        let session = AVAudioSession.sharedInstance()
        // Device check: watchOS manages the route itself during a call. The
        // Bluetooth category options are iOS-only in the SDKs we know of, so
        // the route setting is applied only where the API exists.
        #if os(iOS)
        let options: AVAudioSession.CategoryOptions = settings.route == .speaker ? [] : [.allowBluetooth]
        try session.setCategory(.playAndRecord, mode: .voiceChat, options: options)
        #else
        try session.setCategory(.playAndRecord, mode: .voiceChat, policy: .default, options: [])
        #endif
    }

    private func startAudio() {
        guard !engineRunning else { return }
        let input = engine.inputNode
        let inputFormat = input.outputFormat(forBus: 0)
        let encoder = OpusEncoder.available && codec == .opus ? OpusEncoder() : nil
        let framer = CaptureFramer(from: inputFormat, encoder: encoder)
        let gate = capture
        input.installTap(onBus: 0, bufferSize: 1024, format: inputFormat) { [weak self] buffer, _ in
            guard gate.isOpen else { return }
            for frame in framer.frames(from: buffer) {
                Task { @MainActor in self?.sendFrame(frame) }
            }
        }
        let format = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: Double(voiceSampleRate), channels: 1, interleaved: false)!
        let mixer = playback
        let node = AVAudioSourceNode(format: format) { _, _, frameCount, audioBufferList in
            let buffers = UnsafeMutableAudioBufferListPointer(audioBufferList)
            if let data = buffers.first?.mData?.assumingMemoryBound(to: Float.self) {
                mixer.render(into: data, count: Int(frameCount))
            }
            return noErr
        }
        engine.attach(node)
        engine.connect(node, to: engine.mainMixerNode, format: format)
        sourceNode = node
        do {
            try engine.start()
            engineRunning = true
        } catch {
            fail("Audio could not start: \(error.localizedDescription)")
        }
    }

    private func stopAudio() {
        capture.set(false)
        guard engineRunning || sourceNode != nil else { return }
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        if let node = sourceNode { engine.detach(node) }
        sourceNode = nil
        engineRunning = false
    }

    // MARK: CallKit

    private func startCall() {
        #if canImport(CallKit)
        if provider == nil {
            let config = CXProviderConfiguration()
            config.supportsVideo = false
            config.maximumCallGroups = 1
            config.maximumCallsPerCallGroup = 1
            config.supportedHandleTypes = [.generic]
            let p = CXProvider(configuration: config)
            p.setDelegate(self, queue: nil)
            provider = p
        }
        let id = UUID()
        callId = id
        let action = CXStartCallAction(call: id, handle: CXHandle(type: .generic, value: "GS crew voice"))
        callController.request(CXTransaction(action: action)) { [weak self] error in
            guard let error else { return }
            Task { @MainActor in
                // Without the call voice still works while the app is in front.
                NSLog("[voice] CallKit start failed: \(error.localizedDescription); running foreground-only")
                self?.startWithoutCall()
            }
        }
        #else
        startWithoutCall()
        #endif
    }

    private func startWithoutCall() {
        do {
            try configureSession()
            try AVAudioSession.sharedInstance().setActive(true)
        } catch {
            fail("Audio could not start: \(error.localizedDescription)")
            return
        }
        startAudio()
        connect()
    }

    private func reportConnected() {
        #if canImport(CallKit)
        if let id = callId { provider?.reportOutgoingCall(with: id, connectedAt: nil) }
        #endif
    }

    private func endCall() {
        #if canImport(CallKit)
        guard let id = callId else {
            try? AVAudioSession.sharedInstance().setActive(false)
            return
        }
        callId = nil
        callController.request(CXTransaction(action: CXEndCallAction(call: id))) { _ in }
        #else
        try? AVAudioSession.sharedInstance().setActive(false)
        #endif
    }
}

#if canImport(CallKit)
extension VoiceSession: CXProviderDelegate {
    nonisolated func providerDidReset(_ provider: CXProvider) {
        Task { @MainActor in self.leave() }
    }

    nonisolated func provider(_ provider: CXProvider, perform action: CXStartCallAction) {
        Task { @MainActor in
            do {
                try self.configureSession()
                action.fulfill()
                provider.reportOutgoingCall(with: action.callUUID, startedConnectingAt: nil)
            } catch {
                action.fail()
                self.fail("Audio could not start: \(error.localizedDescription)")
            }
        }
    }

    nonisolated func provider(_ provider: CXProvider, perform action: CXEndCallAction) {
        action.fulfill()
        // Ended from the system call UI: leave voice too.
        Task { @MainActor in if self.callId == action.callUUID { self.callId = nil; self.leave() } }
    }

    nonisolated func provider(_ provider: CXProvider, perform action: CXSetMutedCallAction) {
        action.fulfill()
        // Muting from the system call UI stops talking (like releasing PTT).
        if action.isMuted { Task { @MainActor in self.apply(self.ptt.handle(.timeout)) } }
    }

    /// CallKit activated the audio session: start audio and connect.
    nonisolated func provider(_ provider: CXProvider, didActivate audioSession: AVAudioSession) {
        Task { @MainActor in
            guard !self.userLeft else { return }
            self.startAudio()
            self.connect()
        }
    }

    nonisolated func provider(_ provider: CXProvider, didDeactivate audioSession: AVAudioSession) {
        Task { @MainActor in self.stopAudio() }
    }
}
#endif

// MARK: - Audio helpers (used from the audio threads)

/// Whether captured frames are sent (push-to-talk pressed).
final class CaptureGate: @unchecked Sendable {
    private let lock = NSLock()
    private var open = false
    var isOpen: Bool { lock.lock(); defer { lock.unlock() }; return open }
    func set(_ value: Bool) { lock.lock(); open = value; lock.unlock() }
}

/// WatchCore `VoiceMixer` behind a lock, rendered as Float at 16 kHz.
final class PlaybackMixer: @unchecked Sendable {
    private let lock = NSLock()
    private var mixer = VoiceMixer()
    private var pending: [Int16] = []

    func push(_ speaker: UInt32, _ samples: [Int16]) { lock.lock(); mixer.push(speaker, samples); lock.unlock() }
    func remove(_ speaker: UInt32) { lock.lock(); mixer.remove(speaker); lock.unlock() }
    func clear() { lock.lock(); mixer.clear(); pending.removeAll(); lock.unlock() }
    func setVolume(_ pct: Int) { lock.lock(); mixer.volumePct = pct; lock.unlock() }

    func render(into out: UnsafeMutablePointer<Float>, count: Int) {
        lock.lock()
        defer { lock.unlock() }
        var written = 0
        while written < count {
            if pending.isEmpty { pending = mixer.pull() }
            let n = min(count - written, pending.count)
            for i in 0..<n { out[written + i] = Float(pending[i]) / 32768 }
            pending.removeFirst(n)
            written += n
        }
    }
}

/// Converts microphone buffers to 16 kHz mono Int16, cuts 320-sample frames
/// and encodes them (Opus, or PCM16 bytes).
final class CaptureFramer: @unchecked Sendable {
    private let converter: AVAudioConverter?
    private let target = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: Double(voiceSampleRate), channels: 1, interleaved: true)!
    private let encoder: OpusEncoder?
    private var carry: [Int16] = []

    init(from input: AVAudioFormat, encoder: OpusEncoder?) {
        converter = AVAudioConverter(from: input, to: target)
        self.encoder = encoder
    }

    func frames(from buffer: AVAudioPCMBuffer) -> [Data] {
        guard let converter else { return [] }
        let ratio = target.sampleRate / buffer.format.sampleRate
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio) + 32
        guard let out = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else { return [] }
        var supplied = false
        var error: NSError?
        converter.convert(to: out, error: &error) { _, status in
            if supplied {
                status.pointee = .noDataNow
                return nil
            }
            supplied = true
            status.pointee = .haveData
            return buffer
        }
        guard error == nil, let channel = out.int16ChannelData?[0] else { return [] }
        carry.append(contentsOf: UnsafeBufferPointer(start: channel, count: Int(out.frameLength)))
        var frames: [Data] = []
        while carry.count >= voiceFrameSamples {
            let samples = Array(carry.prefix(voiceFrameSamples))
            carry.removeFirst(voiceFrameSamples)
            if let encoder {
                if let packet = encoder.encode(samples) { frames.append(packet) }
            } else {
                frames.append(encodePcm16(samples))
            }
        }
        return frames
    }
}

private let pcmFormat16k = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: Double(voiceSampleRate), channels: 1, interleaved: true)!

private func opusFormat() -> AVAudioFormat? {
    var asbd = AudioStreamBasicDescription(
        mSampleRate: Double(voiceSampleRate), mFormatID: kAudioFormatOpus, mFormatFlags: 0,
        mBytesPerPacket: 0, mFramesPerPacket: UInt32(voiceFrameSamples), mBytesPerFrame: 0,
        mChannelsPerFrame: 1, mBitsPerChannel: 0, mReserved: 0)
    return AVAudioFormat(streamDescription: &asbd)
}

/// One 320-sample PCM frame to one Opus packet. Device check: watchOS must
/// provide the Opus encoder in AudioToolbox at 16 kHz; when it does not,
/// `available` is false and the session uses PCM16.
final class OpusEncoder: @unchecked Sendable {
    static let available: Bool = OpusEncoder() != nil

    private let converter: AVAudioConverter
    private let format: AVAudioFormat

    init?() {
        guard let format = opusFormat(), let converter = AVAudioConverter(from: pcmFormat16k, to: format) else { return nil }
        converter.bitRate = 24_000
        self.converter = converter
        self.format = format
    }

    func encode(_ samples: [Int16]) -> Data? {
        guard let input = AVAudioPCMBuffer(pcmFormat: pcmFormat16k, frameCapacity: AVAudioFrameCount(samples.count)),
              let channel = input.int16ChannelData?[0]
        else { return nil }
        input.frameLength = AVAudioFrameCount(samples.count)
        samples.withUnsafeBufferPointer { channel.update(from: $0.baseAddress!, count: samples.count) }
        let output = AVAudioCompressedBuffer(format: format, packetCapacity: 1, maximumPacketSize: maxOpusBytes)
        var supplied = false
        var error: NSError?
        let status = converter.convert(to: output, error: &error) { _, inStatus in
            if supplied {
                inStatus.pointee = .noDataNow
                return nil
            }
            supplied = true
            inStatus.pointee = .haveData
            return input
        }
        guard status != .error, error == nil, output.packetCount > 0,
              let description = output.packetDescriptions?[0]
        else { return nil }
        let start = Int(description.mStartOffset)
        let size = Int(description.mDataByteSize)
        guard size > 0, size <= maxOpusBytes else { return nil }
        return Data(bytes: output.data.advanced(by: start), count: size)
    }
}

/// One speaker's Opus stream to 320-sample PCM frames.
final class OpusDecoder {
    private let converter: AVAudioConverter
    private let format: AVAudioFormat

    init?() {
        guard let format = opusFormat(), let converter = AVAudioConverter(from: format, to: pcmFormat16k) else { return nil }
        self.converter = converter
        self.format = format
    }

    func decode(_ packet: Data) -> [Int16]? {
        let input = AVAudioCompressedBuffer(format: format, packetCapacity: 1, maximumPacketSize: max(packet.count, 1))
        packet.withUnsafeBytes { input.data.copyMemory(from: $0.baseAddress!, byteCount: packet.count) }
        input.byteLength = UInt32(packet.count)
        input.packetCount = 1
        input.packetDescriptions?[0] = AudioStreamPacketDescription(
            mStartOffset: 0, mVariableFramesInPacket: 0, mDataByteSize: UInt32(packet.count))
        guard let output = AVAudioPCMBuffer(pcmFormat: pcmFormat16k, frameCapacity: AVAudioFrameCount(voiceFrameSamples)) else { return nil }
        var supplied = false
        var error: NSError?
        let status = converter.convert(to: output, error: &error) { _, inStatus in
            if supplied {
                inStatus.pointee = .noDataNow
                return nil
            }
            supplied = true
            inStatus.pointee = .haveData
            return input
        }
        guard status != .error, error == nil, let channel = output.int16ChannelData?[0],
              Int(output.frameLength) == voiceFrameSamples
        else { return nil }
        return Array(UnsafeBufferPointer(start: channel, count: voiceFrameSamples))
    }
}
