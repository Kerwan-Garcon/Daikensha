#if ANDROID_LINK
import Foundation
import Synchronization

// MARK: - Android link: one protocol v1 conversation, over any byte stream
//
// The port of `run_with` / `read_loop` (server.rs). The transport is TLS from Network.framework on
// the Mac (LinkServer), decrypted relay frames later, or a plain socket in the Linux tests: this
// file does not know which. Everything happens on one serial queue.
//
// What it refuses, on purpose:
//   * a first message that is not a valid `hello` within the ticket's wait (10 s, 3 s when the pool fills);
//   * a wrong token or protocol version, after a short pause, with `error auth`;
//   * a line over 64 KiB (`error protocol`), a line that is not JSON, or silence for 20 minutes.
// It never touches the agent: a decision goes through `LinkHub.decide`, which only applies the
// request still pending.

/// A connected byte stream.
protocol LinkTransport: AnyObject, Sendable {
    /// Sends bytes; `done(true)` once they are handed to the network, `done(false)` if that failed.
    func send(_ bytes: Data, done: @escaping @Sendable (Bool) -> Void)
    /// Closes after what was sent is flushed. Called once.
    func close()
}

/// Which optional features the user has switched on, read again for every connection.
protocol LinkFeatures: Sendable {
    var details: Bool { get }
    var answers: Bool { get }
    var diffs: Bool { get }
    var usage: Bool { get }
    var relay: Bool { get }
    var services: [String] { get }
}

extension LinkFeatures {
    var details: Bool { false }
    var answers: Bool { false }
    var diffs: Bool { false }
    var usage: Bool { false }
    var relay: Bool { false }
    var services: [String] { [] }
}

/// Nothing optional switched on.
struct NoLinkFeatures: LinkFeatures {}

/// What every conversation needs to know about this computer.
final class LinkShared: Sendable {
    let hub: LinkHub
    /// Replaced when the user pairs again; read for every `hello`.
    let token: Mutex<String>
    let name: String
    let os: String
    let features: LinkFeatures
    let admission = Admission()
    let clock: @Sendable () -> UInt64
    let helloTimeout: TimeInterval
    let idleTimeout: TimeInterval
    let authPenalty: TimeInterval

    init(hub: LinkHub, token: String, name: String, os: String = "macos",
         features: LinkFeatures = NoLinkFeatures(),
         clock: @escaping @Sendable () -> UInt64 = { LinkProtocol.nowMs() },
         helloTimeout: TimeInterval = LinkProtocol.helloTimeout,
         idleTimeout: TimeInterval = LinkProtocol.idleTimeout,
         authPenalty: TimeInterval = LinkProtocol.authPenalty) {
        self.hub = hub
        self.token = Mutex(token)
        self.name = name
        self.os = os
        self.features = features
        self.clock = clock
        self.helloTimeout = helloTimeout
        self.idleTimeout = idleTimeout
        self.authPenalty = authPenalty
    }

    var currentToken: String { token.withLock { $0 } }

    func replaceToken(_ new: String) { token.withLock { $0 = new } }
}

/// A message from the phone. Every field is optional and a field of the wrong type reads as absent,
/// like `msg["x"].as_str().unwrap_or("")` in the reference: only a line that is not JSON at all ends
/// the conversation.
struct LinkIncoming: Decodable {
    var type: String?
    var v: Int?
    var token: String?
    var caps: [String]?
    var fingerprint: String?
    var decision: String?

    private enum Key: String, CodingKey {
        case type, v, token, caps, fingerprint, decision
    }

    init() {}

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        type = try? c.decode(String.self, forKey: .type)
        v = try? c.decode(Int.self, forKey: .v)
        token = try? c.decode(String.self, forKey: .token)
        caps = try? c.decode([String].self, forKey: .caps)
        fingerprint = try? c.decode(String.self, forKey: .fingerprint)
        decision = try? c.decode(String.self, forKey: .decision)
    }

    /// nil when the line is not JSON; an empty message when it is JSON but not an object.
    static func parse(_ line: [UInt8]) -> LinkIncoming? {
        do {
            return try JSONDecoder().decode(LinkIncoming.self, from: Data(line))
        } catch DecodingError.typeMismatch {
            return LinkIncoming()
        } catch {
            return nil
        }
    }
}

final class Conversation: LinkSink, @unchecked Sendable {
    private enum Phase {
        case hello
        case talking(id: UInt64, evicted: EvictionFlag, caps: LinkCaps)
        case ending
    }

    private struct Welcome: Encodable {
        let type = "welcome"
        let v: Int
        let desktop: String
        let os: String
        let caps: [String]?
    }

    private struct Pong: Encodable { let type = "pong" }

    private struct ErrorLine: Encodable {
        let type = "error"
        let code: String
        let message: String
    }

    private let shared: LinkShared
    private let transport: LinkTransport
    private let ticket: Ticket?
    private let queue: DispatchQueue

    // Owned by `queue`.
    private var phase = Phase.hello
    private var reader = LineReader()
    private var timer: DispatchWorkItem?
    private var outbox: [LinkOut] = []
    private var sending = false
    private var finished = false
    private var closeAfterFlush = false

    /// Lines accepted for sending and not yet handed to the network (any thread).
    private let pending = Mutex(0)

    /// Called once, on `queue`, when the conversation is over (the transport is closed).
    var onFinish: (@Sendable () -> Void)?

    init(shared: LinkShared, transport: LinkTransport, ticket: Ticket?, queue: DispatchQueue) {
        self.shared = shared
        self.transport = transport
        self.ticket = ticket
        self.queue = queue
    }

    /// Starts the clock for the hello. Call on `queue`, once the transport is ready.
    func start() {
        // 10 s, or 3 s from the ticket when the pool is filling up (both equal the reference's).
        arm(min(ticket?.wait ?? shared.helloTimeout, shared.helloTimeout))
    }

    /// Bytes from the phone. Call on `queue`.
    func received(_ bytes: Data) {
        guard !finished else { return }
        for next in reader.feed(bytes) {
            switch next {
            case .tooLong:
                say(.close(code: "protocol", message: "line too long"))
                end()
                return
            case .line(let line):
                handle(line)
            }
            if case .ending = phase { return }
        }
    }

    /// The phone closed, or the transport failed. Call on `queue`.
    func receivedEnd() {
        guard !finished else { return }
        end()
        finish()
    }

    // MARK: Lines

    private func handle(_ line: [UInt8]) {
        switch phase {
        case .hello:
            hello(line)
        case .talking(let id, let evicted, let caps):
            talk(line, id: id, evicted: evicted, caps: caps)
        case .ending:
            break
        }
    }

    private func hello(_ line: [UInt8]) {
        cancelTimer()
        guard let msg = LinkIncoming.parse(line), msg.type == "hello" else {
            end()
            return
        }
        let expected = shared.currentToken
        let good = msg.v == LinkProtocol.version
            && LinkCrypto.constantTimeEqual(Array((msg.token ?? "").utf8), Array(expected.utf8))
        guard good else {
            phase = .ending
            queue.asyncAfter(deadline: .now() + shared.authPenalty) { [self] in
                say(.close(code: "auth", message: "bad token or version"))
                end()
            }
            return
        }
        // From here on this connection no longer counts against the unauthenticated limits.
        ticket?.authenticated()
        let asked = Set(msg.caps ?? [])
        var caps = LinkCaps()
        // The outfit is no secret and says nothing about the user's work: offered whenever the phone asks.
        caps.prefs = asked.contains("prefs")
        caps.details = asked.contains("details") && shared.features.details
        caps.answers = asked.contains("answers") && shared.features.answers
        caps.diffs = asked.contains("diffs") && shared.features.diffs
        caps.usage = asked.contains("usage") && shared.features.usage
        caps.relay = asked.contains("relay") && shared.features.relay
        caps.services = asked.contains("services") ? shared.features.services : []
        let offered = Self.offered(caps)
        say(.line(LinkProtocol.line(Welcome(v: LinkProtocol.version, desktop: shared.name, os: shared.os,
                                            caps: offered.isEmpty ? nil : offered))))
        let (id, evicted) = shared.hub.subscribe(self, now: shared.clock(), caps: caps)
        phase = .talking(id: id, evicted: evicted, caps: caps)
        arm(shared.idleTimeout)
    }

    /// The capabilities a welcome lists, in the reference's order. Only those this build implements.
    static func offered(_ caps: LinkCaps) -> [String] {
        var out: [String] = []
        if caps.details { out.append("details") }
        if caps.answers { out.append("answers") }
        if caps.prefs { out.append("prefs") }
        if caps.diffs { out.append("diffs") }
        if caps.usage { out.append("usage") }
        if caps.relay { out.append("relay") }
        if !caps.services.isEmpty { out.append("services") }
        return out
    }

    private func talk(_ line: [UInt8], id: UInt64, evicted: EvictionFlag, caps: LinkCaps) {
        arm(shared.idleTimeout)
        guard let msg = LinkIncoming.parse(line), !evicted.isSet else {
            end()
            return
        }
        switch msg.type {
        case "ping":
            say(.line(LinkProtocol.line(Pong())))
        case "decision":
            // The hub tells every phone when it applies one; an ignored decision changes nothing at all.
            shared.hub.decide(fingerprint: msg.fingerprint ?? "", decision: msg.decision ?? "", now: shared.clock())
        case "bye":
            end()
        default:
            break   // unknown types are ignored
        }
    }

    // MARK: Timers

    private func arm(_ seconds: TimeInterval) {
        cancelTimer()
        let item = DispatchWorkItem { [weak self] in
            guard let self, !self.finished else { return }
            self.end()   // silent for too long
        }
        timer = item
        queue.asyncAfter(deadline: .now() + seconds, execute: item)
    }

    private func cancelTimer() {
        timer?.cancel()
        timer = nil
    }

    // MARK: Ending

    /// No more reading; what is queued is still sent (a last `error` included), then the transport closes.
    private func end() {
        cancelTimer()
        if case .talking(let id, _, _) = phase {
            shared.hub.unsubscribe(id)
        }
        phase = .ending
        closeAfterFlush = true
        pump()
    }

    private func finish() {
        guard !finished else { return }
        finished = true
        cancelTimer()
        if case .talking(let id, _, _) = phase { shared.hub.unsubscribe(id) }
        phase = .ending
        outbox.removeAll()
        transport.close()
        ticket?.release()
        onFinish?()
    }

    // MARK: Writing

    /// LinkSink: from the hub, any thread. Never blocks.
    func offer(_ out: LinkOut) -> Bool {
        let accepted = pending.withLock { n -> Bool in
            guard n < LinkProtocol.outQueueLimit else { return false }
            n += 1
            return true
        }
        guard accepted else { return false }
        queue.async { [self] in
            guard !finished else { return }
            outbox.append(out)
            pump()
        }
        return true
    }

    /// From this conversation itself, on `queue`.
    private func say(_ out: LinkOut) {
        pending.withLock { $0 += 1 }
        outbox.append(out)
        pump()
    }

    private func pump() {
        guard !finished, !sending else { return }
        guard !outbox.isEmpty else {
            if closeAfterFlush { finish() }
            return
        }
        let out = outbox.removeFirst()
        let text: String
        let last: Bool
        switch out {
        case .line(let l):
            text = l
            last = false
        case .close(let code, let message):
            text = LinkProtocol.line(ErrorLine(code: code, message: message))
            last = true
        }
        sending = true
        transport.send(Data((text + "\n").utf8)) { [self] ok in
            queue.async { [self] in
                pending.withLock { $0 -= 1 }
                sending = false
                if !ok || last {
                    if case .talking(let id, _, _) = phase { shared.hub.unsubscribe(id) }
                    finish()
                } else {
                    pump()
                }
            }
        }
    }
}
#endif
