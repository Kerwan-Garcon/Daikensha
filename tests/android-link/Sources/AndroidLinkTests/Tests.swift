import Foundation
import Synchronization
#if canImport(CryptoKit)
import CryptoKit
#else
import Crypto
#endif

// The Android link's portable core, tested the way windows/src-tauri/src/phone_link tests the
// reference: each case below names the Rust test it mirrors (admission.rs, hub.rs, server.rs,
// pairing.rs, discovery.rs, tests.rs). Run by scripts/test-android-link.sh, in Docker or on a Mac.

nonisolated(unsafe) var failures: [String] = []
nonisolated(unsafe) var passed = 0
nonisolated(unsafe) var current = ""

func check(_ ok: Bool, _ what: @autoclosure () -> String = "", line: Int = #line) {
    if !ok { failures.append("\(current) (line \(line)) \(what())") }
}

func test(_ name: String, _ body: () throws -> Void) {
    current = name
    let before = failures.count
    do { try body() } catch { failures.append("\(name): threw \(error)") }
    if failures.count == before { passed += 1 }
}

/// Waits until `cond` holds, at most `seconds`.
func eventually(_ seconds: Double = 3, _ cond: () -> Bool) -> Bool {
    let deadline = Date().addingTimeInterval(seconds)
    while Date() < deadline {
        if cond() { return true }
        usleep(10_000)
    }
    return cond()
}

// MARK: - Fixtures

final class Recorder: LinkHost {
    let decisions = Mutex<[(String, Bool)]>([])
    func decide(requestId: String, allow: Bool) { decisions.withLock { $0.append((requestId, allow)) } }
    var all: [(String, Bool)] { decisions.withLock { $0 } }
}

/// A sink that keeps what it is offered, up to a capacity (like the bounded channel of the reference).
final class Sink: LinkSink {
    let capacity: Int
    let items = Mutex<[LinkOut]>([])
    init(capacity: Int = 64) { self.capacity = capacity }
    func offer(_ out: LinkOut) -> Bool {
        items.withLock { items in
            guard items.count < capacity else { return false }
            items.append(out)
            return true
        }
    }
    /// The JSON lines received since the last drain.
    func drain() -> [[String: Any]] {
        let taken = items.withLock { i -> [LinkOut] in defer { i = [] }; return i }
        return taken.compactMap {
            if case .line(let l) = $0 { return (try? JSONSerialization.jsonObject(with: Data(l.utf8))) as? [String: Any] }
            return nil
        }
    }
    var closes: [LinkOut] { items.withLock { $0.filter { if case .close = $0 { return true } else { return false } } } }
}

func approval(_ request: String, _ command: String) -> ApprovalIn {
    ApprovalIn(requestId: request, sessionId: "s1", pillId: "integration_claude", tool: "Bash", command: command)
}

func session(_ state: String) -> SessionIn {
    SessionIn(pillId: "integration_claude", agent: "Claude Code", state: state)
}

func types(_ lines: [[String: Any]]) -> [String] { lines.compactMap { $0["type"] as? String } }

/// A server on loopback with an in-memory pairing.
final class Rig {
    let recorder = Recorder()
    let hub: LinkHub
    let shared: LinkShared
    let server: PosixServer
    static let token = "tok-0123456789abcdefghij"

    init(helloTimeout: TimeInterval = LinkProtocol.helloTimeout, idleTimeout: TimeInterval = LinkProtocol.idleTimeout,
         clock: @escaping @Sendable () -> UInt64 = { LinkProtocol.nowMs() }) throws {
        hub = LinkHub(host: recorder)
        shared = LinkShared(hub: hub, token: Rig.token, name: "Test Mac", clock: clock,
                            helloTimeout: helloTimeout, idleTimeout: idleTimeout, authPenalty: 0.05)
        server = try PosixServer(shared: shared)
    }

    deinit { server.stop() }

    func paired(caps: [String]? = nil) -> TestClient? {
        guard let c = TestClient(port: server.port) else { return nil }
        c.hello(token: Rig.token, caps: caps)
        return c.expect("welcome") == nil ? nil : c
    }

    /// The fingerprint of the request the hub holds.
    var fingerprint: String { hub.pendingFingerprint ?? "" }
}

// MARK: - The tests

@main
enum AndroidLinkTests {
    static func main() {
        admissionTests()
        lineReaderTests()
        addressTests()
        hubTests()
        pairingTests()
        certificateTests()
        conversationTests()
        if failures.isEmpty {
            print("android link: \(passed) tests passed")
        } else {
            for f in failures { print("FAIL \(f)") }
            print("android link: \(failures.count) failure(s), \(passed) tests passed")
            exit(1)
        }
    }

    // MARK: admission.rs

    static func admissionTests() {
        test("one_address_holds_at_most_two_unauthenticated_connections") {
            let a = Admission()
            var t1 = a.admit("192.168.1.50")
            let t2 = a.admit("192.168.1.50")
            check(t1 != nil && t2 != nil)
            check(a.admit("192.168.1.50") == nil, "the third from one address")
            check(a.admit("192.168.1.51") != nil, "another address is not affected")
            t1 = nil
            check(a.admit("192.168.1.50") != nil, "a closed connection frees its place")
            _ = t2
        }
        test("the_same_address_written_two_ways_is_one_address") {
            let a = Admission()
            let t1 = a.admit(LinkAddress.key("192.168.1.50"))
            let t2 = a.admit(LinkAddress.key("::ffff:192.168.1.50"))
            check(t1 != nil && t2 != nil)
            check(a.admit(LinkAddress.key("192.168.1.50")) == nil)
        }
        test("strangers_share_a_pool_smaller_than_the_whole") {
            let a = Admission()
            var held: [Ticket] = []
            for n in 0..<(Admission.maxUnauthStrangers / Admission.maxUnauthPerAddress) {
                for _ in 0..<Admission.maxUnauthPerAddress { if let t = a.admit("10.0.0.\(n + 1)") { held.append(t) } }
            }
            check(held.count == Admission.maxUnauthStrangers)
            check(a.admit("10.0.0.99") == nil, "the stranger pool is full")
            check(held.count < Admission.maxConnections, "the pool leaves slots for known phones")
        }
        test("a_flood_from_many_addresses_cannot_lock_out_the_paired_phone") {
            let a = Admission()
            a.admit("192.168.1.20")!.authenticated()
            check(a.knows("192.168.1.20"))
            var flood: [Ticket] = []
            for n in 0..<50 { if let t = a.admit("192.168.1.\(100 + n)") { flood.append(t) } }
            check(flood.count == Admission.maxUnauthStrangers, "the flood is capped")
            let phone = a.admit("192.168.1.20")
            check(phone != nil, "the paired phone is not locked out")
            phone?.authenticated()
            let again = a.admit("192.168.1.20")
            check(again != nil, "and can reconnect while the flood goes on")
            again?.authenticated()
            check(a.counts.strangers == Admission.maxUnauthStrangers)
            check(a.counts.total <= Admission.maxConnections)
        }
        test("a_flood_from_one_address_leaves_every_other_address_alone") {
            let a = Admission()
            let flood = (0..<20).compactMap { _ in a.admit("192.168.1.66") }
            let newcomer = a.admit("192.168.1.20")
            check(newcomer != nil, "a new phone, never seen before, still connects")
            check(a.counts.strangers == Admission.maxUnauthPerAddress + 1)
            _ = flood
        }
        test("authenticating_frees_the_unauthenticated_places_and_is_counted_once") {
            let a = Admission()
            var t = a.admit("192.168.1.20")
            t?.authenticated()
            t?.authenticated()
            check(a.counts == (1, 0))
            let u1 = a.admit("192.168.1.20"), u2 = a.admit("192.168.1.20")
            check(u1 != nil && u2 != nil, "an authenticated connection no longer counts against the two-per-address limit")
            t = nil
            check(a.counts.total == 2)
        }
        test("the_places_come_back_when_tickets_drop_whatever_their_state") {
            let a = Admission()
            do {
                let t1 = a.admit("192.168.1.20")
                let t2 = a.admit("192.168.1.21")
                t1?.authenticated()
                _ = t2
            }
            check(a.counts == (0, 0))
            check(a.admit("192.168.1.21") != nil)
        }
        test("the_total_never_goes_over_eight_even_for_known_phones") {
            let a = Admission()
            var held: [Ticket] = []
            for n in 0..<Admission.maxConnections {
                let t = a.admit("192.168.1.\(10 + n)")!
                t.authenticated()
                held.append(t)
            }
            check(a.admit("192.168.1.10") == nil)
            check(a.counts.total == Admission.maxConnections)
        }
        test("the_wait_is_shorter_when_the_pool_is_filling_up") {
            let a = Admission()
            check(a.admit("192.168.1.1")?.wait == Admission.patient, "an empty server is patient")
            let held = (0..<2).compactMap { a.admit("192.168.2.\($0)") }
            check(a.admit("192.168.3.1")?.wait == Admission.hurried, "two strangers waiting is half of their pool")
            let b = Admission()
            var known: [Ticket] = []
            for n in 0..<6 {
                let t = b.admit("192.168.4.\(n)")!
                t.authenticated()
                known.append(t)
            }
            check(b.admit("192.168.4.0")?.wait == Admission.hurried, "nearly full: even a known phone is hurried")
            _ = held
        }
        test("at_most_eight_addresses_are_remembered") {
            let a = Admission()
            for n in 0..<(Admission.knownAddresses + 3) { a.admit("192.168.5.\(n)")?.authenticated() }
            check(!a.knows("192.168.5.0"), "the oldest is forgotten")
            check(a.knows("192.168.5.\(Admission.knownAddresses + 2)"))
        }
        test("a_released_ticket_is_released_once") {
            let a = Admission()
            let t = a.admit("192.168.1.9")!
            t.release()
            t.release()
            check(a.counts == (0, 0))
        }
    }

    // MARK: server.rs LineReader

    static func lineReaderTests() {
        test("lines_are_cut_at_newlines_with_an_optional_cr_and_empties_skipped") {
            var r = LineReader(limit: 10)
            check(r.feed(Array("ab\r\n\n\ncd".utf8)) == [.line(Array("ab".utf8))])
            check(r.feed(Array("e\n".utf8)) == [.line(Array("cde".utf8))])
        }
        test("a_line_at_the_limit_is_accepted_and_one_more_byte_is_not") {
            var ok = LineReader(limit: 10)
            check(ok.feed(Array(String(repeating: "x", count: 10).utf8) + [13, 10]) == [.line(Array(repeating: 120, count: 10))])
            var over = LineReader(limit: 10)
            check(over.feed(Array(String(repeating: "x", count: 11).utf8) + [10]) == [.tooLong])
            var unterminated = LineReader(limit: 10)
            check(unterminated.feed(Array(repeating: 120, count: 11)) == [], "11 bytes may still end with \\r\\n... no: 11 > 10 but needs the 12th")
            check(unterminated.feed([120]) == [.tooLong], "12 bytes without a newline cannot fit")
            check(unterminated.feed(Array("ok\n".utf8)) == [], "nothing after a protocol error")
        }
        test("the_reader_never_holds_more_than_the_limit") {
            var r = LineReader(limit: 1024)
            let out = r.feed(Array(repeating: 65, count: 1_000_000))
            check(out == [.tooLong])
        }
    }

    // MARK: server.rs is_local

    static func addressTests() {
        test("only_the_local_network_may_connect") {
            for ok in ["192.168.1.20", "10.0.0.5", "172.16.3.4", "172.31.255.255", "127.0.0.1", "169.254.7.7", "::1",
                       "fd12:3456::1", "fe80::1", "fe80::1%en0", "::ffff:192.168.0.9"] {
                check(LinkAddress.isLocal(ok), ok)
            }
            for no in ["8.8.8.8", "172.32.0.1", "100.64.0.1", "2001:db8::1", "::ffff:8.8.8.8", "0.0.0.0", "", "not an address"] {
                check(!LinkAddress.isLocal(no), no)
            }
        }
    }

    // MARK: hub.rs

    static func hubTests() {
        func hub() -> (LinkHub, Recorder) {
            let r = Recorder()
            return (LinkHub(host: r), r)
        }

        test("the_fingerprint_is_the_macs_derivation") {
            let joined = ["integration_claude", "s1", "Bash", "ls", "req-1"].joined(separator: "\u{1F}")
            let expected = LinkCrypto.hex(Array(SHA256.hash(data: Data(joined.utf8))))
            check(LinkProtocol.fingerprint(pillId: "integration_claude", sessionId: "s1", tool: "Bash", command: "ls", inputKey: "req-1") == expected)
            // The value the Kotlin client's Fingerprint.of gives in RustDesktopInteropTest.
            check(LinkProtocol.fingerprint(pillId: "integration_claude", sessionId: "interop", tool: "Bash", command: "npm run build", inputKey: "req-1").count == 64)
        }
        test("a_command_longer_than_the_phone_shows_is_flagged_and_cannot_be_allowed_from_the_phone") {
            let (h, r) = hub()
            let sink = Sink()
            _ = h.subscribe(sink, now: 1_000)
            h.publish(sessions: [], approval: approval("req-1", String(repeating: "é", count: 600)), now: 1_000)
            let a = sink.drain().first { $0["type"] as? String == "approval" }
            check(a?["truncated"] as? Bool == true)
            check((a?["command"] as? String)?.unicodeScalars.count == 500)
            check(h.decide(fingerprint: h.pendingFingerprint!, decision: "allow", now: 1_001) == .tooLong)
            check(r.all.isEmpty, "nothing reached the agent")
            check(h.pendingFingerprint != nil, "the request stays pending, to be answered on the Mac")
            check(h.decide(fingerprint: h.pendingFingerprint!, decision: "deny", now: 1_002) == .applied)
            check(r.all.count == 1 && r.all[0] == ("req-1", false))
            // The fingerprint covers the text as sent.
            let sent = String(repeating: "é", count: 500)
            check(LinkProtocol.fingerprint(pillId: "integration_claude", sessionId: "s1", tool: "Bash", command: sent, inputKey: "req-1") == a?["fingerprint"] as? String)
        }
        test("a_command_that_fits_is_not_flagged_and_can_be_allowed") {
            let (h, r) = hub()
            let sink = Sink()
            _ = h.subscribe(sink, now: 1_000)
            h.publish(sessions: [], approval: approval("req-1", String(repeating: "x", count: 500)), now: 1_000)
            let a = sink.drain().first { $0["type"] as? String == "approval" }
            check(a?["truncated"] as? Bool == false)
            check(h.decide(fingerprint: h.pendingFingerprint!, decision: "allow", now: 1_001) == .applied)
            check(r.all.count == 1 && r.all[0] == ("req-1", true))
        }
        test("a_phone_that_connects_later_is_also_told_the_command_is_cut") {
            let (h, _) = hub()
            h.publish(sessions: [], approval: approval("req-1", String(repeating: "x", count: 501)), now: 1_000)
            let sink = Sink()
            _ = h.subscribe(sink, now: 1_000)
            check(sink.drain().first { $0["type"] as? String == "approval" }?["truncated"] as? Bool == true)
        }
        test("a_matching_decision_is_applied_once") {
            let (h, r) = hub()
            h.publish(sessions: [], approval: approval("req-1", "ls"), now: 1_000)
            let fp = h.pendingFingerprint!
            check(h.decide(fingerprint: fp, decision: "allow", now: 2_000) == .applied)
            check(h.decide(fingerprint: fp, decision: "allow", now: 2_001) == .noMatch)
            check(r.all.count == 1 && r.all[0] == ("req-1", true))
        }
        test("a_fingerprint_that_does_not_match_changes_nothing") {
            let (h, r) = hub()
            h.publish(sessions: [], approval: approval("req-1", "ls"), now: 1_000)
            check(h.decide(fingerprint: String(repeating: "0", count: 64), decision: "allow", now: 1_001) == .noMatch)
            check(h.decide(fingerprint: "", decision: "allow", now: 1_001) == .noMatch)
            check(r.all.isEmpty)
            check(h.pendingFingerprint != nil, "still pending")
        }
        test("a_decision_for_a_replaced_request_is_ignored") {
            let (h, r) = hub()
            h.publish(sessions: [], approval: approval("req-1", "ls"), now: 1_000)
            let old = h.pendingFingerprint!
            h.publish(sessions: [], approval: approval("req-2", "ls"), now: 1_100)
            check(old != h.pendingFingerprint, "the same command asked twice has two fingerprints")
            check(h.decide(fingerprint: old, decision: "allow", now: 1_200) == .noMatch)
            check(r.all.isEmpty)
        }
        test("a_decision_after_the_island_answered_is_ignored") {
            let (h, r) = hub()
            h.publish(sessions: [], approval: approval("req-1", "ls"), now: 1_000)
            let fp = h.pendingFingerprint!
            h.publish(sessions: [], approval: nil, now: 1_100)
            check(h.decide(fingerprint: fp, decision: "allow", now: 1_200) == .noMatch)
            check(r.all.isEmpty)
        }
        test("a_late_decision_is_ignored_and_the_request_dismissed") {
            let (h, r) = hub()
            let sink = Sink()
            _ = h.subscribe(sink, now: 1_000)
            h.publish(sessions: [], approval: approval("req-1", "ls"), now: 1_000)
            let fp = h.pendingFingerprint!
            _ = sink.drain()
            check(h.decide(fingerprint: fp, decision: "allow", now: 1_000 + LinkProtocol.approvalTTLms + 1) == .late)
            check(r.all.isEmpty)
            check(h.pendingFingerprint == nil)
            check(types(sink.drain()) == ["approvalResolved"])
        }
        test("exactly_at_the_limit_is_still_in_time") {
            let (h, r) = hub()
            h.publish(sessions: [], approval: approval("req-1", "ls"), now: 1_000)
            check(h.decide(fingerprint: h.pendingFingerprint!, decision: "deny", now: 1_000 + LinkProtocol.approvalTTLms) == .applied)
            check(r.all.count == 1)
        }
        test("only_allow_and_deny_exist") {
            let (h, r) = hub()
            h.publish(sessions: [], approval: approval("req-1", "ls"), now: 1_000)
            for word in ["always", "ask", "ALLOW", "", "allow "] {
                check(h.decide(fingerprint: h.pendingFingerprint!, decision: word, now: 1_001) == .invalid, word)
            }
            check(r.all.isEmpty)
            check(h.pendingFingerprint != nil)
        }
        test("phones_are_told_what_changed_and_only_that") {
            let (h, _) = hub()
            let sink = Sink()
            _ = h.subscribe(sink, now: 1_000)
            check(types(sink.drain()) == ["sessions"], "a new phone gets the picture")
            h.publish(sessions: [session("working")], approval: nil, now: 1_000)
            let first = sink.drain()
            check(types(first) == ["sessions"])
            h.publish(sessions: [session("working")], approval: nil, now: 2_000)
            check(sink.drain().isEmpty, "the same picture again is not sent")
            h.publish(sessions: [session("thinking")], approval: approval("req-1", "ls"), now: 3_000)
            let next = sink.drain()
            check(types(next) == ["sessions", "approval"])
            let s = (next[0]["sessions"] as? [[String: Any]])?.first
            check(s?["updatedAt"] as? Int == 3_000)
            check(Set(s?.keys.map { $0 } ?? []) == ["pillId", "agent", "state", "statusText", "stepIndex", "stepCount", "updatedAt"],
                  "exactly the v1 fields")
            h.publish(sessions: [session("thinking")], approval: approval("req-1", "ls"), now: 4_000)
            check(sink.drain().isEmpty, "same request, nothing new")
            h.publish(sessions: [session("thinking")], approval: nil, now: 5_000)
            check(types(sink.drain()) == ["approvalResolved"])
        }
        test("an_unchanged_session_keeps_its_time_and_a_changed_one_moves") {
            let (h, _) = hub()
            let sink = Sink()
            h.publish(sessions: [session("working"), SessionIn(pillId: "agent_codex", agent: "Codex", state: "idle")], approval: nil, now: 1_000)
            h.publish(sessions: [session("working"), SessionIn(pillId: "agent_codex", agent: "Codex", state: "working")], approval: nil, now: 2_000)
            _ = h.subscribe(sink, now: 2_000)
            let s = sink.drain()[0]["sessions"] as? [[String: Any]] ?? []
            check(s.first?["updatedAt"] as? Int == 1_000)
            check(s.last?["updatedAt"] as? Int == 2_000)
        }
        test("a_phone_that_connects_late_still_sees_the_pending_request") {
            let (h, _) = hub()
            h.publish(sessions: [session("approval")], approval: approval("req-1", "ls"), now: 1_000)
            let sink = Sink()
            _ = h.subscribe(sink, now: 2_000)
            check(types(sink.drain()) == ["sessions", "approval"])
        }
        test("a_request_that_waited_too_long_is_gone_for_a_phone_that_connects_now") {
            let (h, _) = hub()
            h.publish(sessions: [], approval: approval("req-1", "ls"), now: 1_000)
            let sink = Sink()
            _ = h.subscribe(sink, now: 1_000 + LinkProtocol.approvalTTLms + 1)
            check(types(sink.drain()) == ["sessions"])
            check(h.pendingFingerprint == nil)
        }
        test("what_the_island_sends_is_bounded_and_cleaned") {
            let (h, _) = hub()
            let sink = Sink()
            _ = h.subscribe(sink, now: 0)
            _ = sink.drain()
            var many = (0..<40).map { SessionIn(pillId: "p\($0)", agent: String(repeating: "a", count: 300), state: "nonsense",
                                                statusText: String(repeating: "s", count: 1_000)) }
            many.insert(SessionIn(pillId: "", agent: "no pill"), at: 0)
            h.publish(sessions: many, approval: ApprovalIn(requestId: "r", sessionId: "s", pillId: "p0",
                                                           tool: String(repeating: "t", count: 100), command: "ls"), now: 1)
            let lines = sink.drain()
            let s = lines[0]["sessions"] as? [[String: Any]] ?? []
            check(s.count == LinkProtocol.maxSessions)
            check(s.first?["pillId"] as? String == "p0", "a session without a pill is dropped")
            check(s.first?["state"] as? String == "idle", "an unknown state is sent as idle")
            check((s.first?["agent"] as? String)?.count == 64)
            check((s.first?["statusText"] as? String)?.count == 200)
            check((lines[1]["tool"] as? String)?.count == 64)
            // An approval without an id is not offered.
            let (h2, _) = hub()
            h2.publish(sessions: [], approval: ApprovalIn(requestId: "", sessionId: "s", pillId: "p", tool: "Bash", command: "ls"), now: 1)
            check(h2.pendingFingerprint == nil)
        }
        test("a_phone_that_cannot_keep_up_is_dropped_not_waited_for") {
            let (h, _) = hub()
            let slow = Sink(capacity: 1), ok = Sink()
            let (_, slowEvicted) = h.subscribe(slow, now: 0)   // the first sessions line fills it
            let (_, okEvicted) = h.subscribe(ok, now: 0)
            h.publish(sessions: [session("working")], approval: nil, now: 1)
            check(h.clients == 1)
            check(slowEvicted.isSet, "a phone that was dropped is marked so its connection can close")
            check(!okEvicted.isSet)
        }
        test("kicking_tells_every_phone_why") {
            let (h, _) = hub()
            let a = Sink(), b = Sink()
            _ = h.subscribe(a, now: 0)
            _ = h.subscribe(b, now: 0)
            h.kickAll(code: "auth", message: "unpaired")
            check(a.closes == [.close(code: "auth", message: "unpaired")])
            check(b.closes == [.close(code: "auth", message: "unpaired")])
            check(h.clients == 0)
        }
        test("constant_time_comparison_still_compares") {
            check(LinkCrypto.constantTimeEqual(Array("abc".utf8), Array("abc".utf8)))
            check(!LinkCrypto.constantTimeEqual(Array("abc".utf8), Array("abd".utf8)))
            check(!LinkCrypto.constantTimeEqual(Array("abc".utf8), Array("abcd".utf8)))
            check(LinkCrypto.constantTimeEqual([UInt8](), [UInt8]()))
        }
    }

    // MARK: pairing.rs, discovery.rs

    static func pairingTests() {
        test("the_certificate_is_made_once_and_then_kept") {
            let store = MemoryIdentityStore()
            let first = try LinkPairing.loadOrCreateIdentity(store)
            let again = try LinkPairing.loadOrCreateIdentity(store)
            check(first.certificate == again.certificate)
            check(first.fingerprint == again.fingerprint)
            check(first.fingerprint.count == 64)
            check(first.fingerprint == LinkCrypto.hex(Array(SHA256.hash(data: Data(first.certificate)))), "the SHA-256 of the DER, which the phone pins")
        }
        test("a_damaged_key_means_a_new_identity_not_a_crash") {
            let store = MemoryIdentityStore()
            let first = try LinkPairing.loadOrCreateIdentity(store)
            try store.save(LinkIdentity(certificate: first.certificate, privateKey: Data([1, 2, 3])))
            let next = try LinkPairing.loadOrCreateIdentity(store)
            check(first.fingerprint != next.fingerprint)
            check(try LinkPairing.loadOrCreateIdentity(store).fingerprint == next.fingerprint, "the new one is kept")
        }
        test("tokens_are_valid_random_and_replaced_on_demand") {
            let store = MemorySecretStore()
            let a = try LinkPairing.token(store)
            check(LinkPairing.isToken(a), a)
            check(a.count == 32)
            check(try LinkPairing.token(store) == a, "the token is kept")
            let b = try LinkPairing.newToken(store)
            check(LinkPairing.isToken(b) && a != b)
            check(try LinkPairing.token(store) == b)
            try store.set(LinkPairing.tokenKey, "short")
            check(LinkPairing.isToken(try LinkPairing.token(store)), "a damaged stored token is replaced rather than used")
        }
        test("a_keychain_that_refuses_writes_stops_the_pairing") {
            let store = MemorySecretStore()
            store.refuseWrites.store(true, ordering: .sequentiallyConsistent)
            check((try? LinkPairing.token(store)) == nil)
        }
        test("the_token_alphabet_matches_the_protocol") {
            check(LinkPairing.isToken(String(repeating: "a", count: 16)))
            check(LinkPairing.isToken(String(repeating: "A-_9", count: 32)))
            check(!LinkPairing.isToken(String(repeating: "a", count: 15)))
            check(!LinkPairing.isToken(String(repeating: "a", count: 129)))
            check(!LinkPairing.isToken("aaaaaaaaaaaaaaa!"))
            check(!LinkPairing.isToken("aaaaaaaaaaaaaaa "))
            check(!LinkPairing.isToken("aaaaaaaaaaaaaaaé"))
            check(!LinkPairing.isToken(""))
        }
        test("the_link_has_every_field_and_encodes_the_name") {
            let link = LinkPairing.link(host: "192.168.1.20", port: 47821, fingerprint: String(repeating: "ab", count: 32),
                                        token: "tok_en-1234567890abcd", name: "Léa's PC & co")
            check(link == "coucou://pair?v=1&host=192.168.1.20&port=47821&fp=\(String(repeating: "ab", count: 32))&token=tok_en-1234567890abcd&name=L%C3%A9a%27s%20PC%20%26%20co", link)
        }
        test("the_longest_realistic_link_still_fits_what_the_phone_accepts") {
            let name = LinkPairing.displayName(String(repeating: "é", count: 60))
            check(name.count == 48)
            let link = LinkPairing.link(host: "2001:db8:aaaa:bbbb:cccc:dddd:eeee:ffff", port: 47821, fingerprint: String(repeating: "ab", count: 32),
                                        token: String(repeating: "t", count: 128), name: name)
            check(link.utf8.count < 2048, "PairingScan.MAX_LINK")
            check(LinkPairing.displayName("   ") == "Coucou")
        }
        test("the_txt_record_is_the_version_and_the_id_and_nothing_else") {
            let fp = "AB12CD34EF56AB78901234567890abcdef1234567890abcdef1234567890abcd"
            check(LinkDiscovery.fingerprintId(fp) == "ab12cd34ef56ab78")
            check(LinkDiscovery.txt(fp) == ["v": "1", "fp": "ab12cd34ef56ab78"])
            check(LinkDiscovery.fingerprintId("abc") == nil)
            check(LinkDiscovery.fingerprintId("zz12cd34ef56ab78901234") == nil)
        }
        test("nothing_secret_can_reach_the_announcement") {
            let token = "tok_ABCDEFGHIJKLMNOP1234"
            let fp = String(repeating: "ab", count: 32)
            let link = LinkPairing.link(host: "192.168.1.20", port: 47821, fingerprint: fp, token: token, name: "Mac")
            let said = (LinkDiscovery.txt(fp) ?? [:]).map { "\($0.key)=\($0.value)" }.joined() + LinkDiscovery.instanceName("Mac")
            check(!said.contains(token) && !said.contains(link) && !said.contains(fp))
            check(LinkDiscovery.txt(token) == nil, "a token is not a fingerprint")
        }
        test("the_instance_is_the_computer_name_cut_to_one_label") {
            check(LinkDiscovery.instanceName("Louis's MacBook") == "Louis's MacBook")
            check(LinkDiscovery.instanceName(String(repeating: "é", count: 40)).utf8.count <= 63)
            check(LinkDiscovery.instanceName("") == "Coucou")
        }
    }

    // MARK: the certificate (what rcgen made on Windows)

    /// The elements of a DER SEQUENCE: (tag, whole TLV bytes, content).
    static func derElements(_ bytes: [UInt8]) -> [(UInt8, [UInt8], [UInt8])] {
        var out: [(UInt8, [UInt8], [UInt8])] = []
        var i = 0
        while i < bytes.count {
            let tag = bytes[i]
            var len = Int(bytes[i + 1])
            var head = 2
            if len & 0x80 != 0 {
                let n = len & 0x7F
                len = 0
                for k in 0..<n { len = len << 8 | Int(bytes[i + 2 + k]) }
                head += n
            }
            out.append((tag, Array(bytes[i..<(i + head + len)]), Array(bytes[(i + head)..<(i + head + len)])))
            i += head + len
        }
        return out
    }

    static func certificateTests() {
        test("the_certificate_is_signed_by_its_own_key_and_carries_it") {
            let id = try LinkIdentity.generate()
            let outer = derElements(id.certificate)
            check(outer.count == 1 && outer[0].0 == 0x30, "one SEQUENCE")
            let parts = derElements(outer[0].2)
            check(parts.count == 3, "tbs, algorithm, signature")
            let tbs = parts[0].1
            let sig = Array(parts[2].2.dropFirst())   // BIT STRING: no unused bits
            let key = id.signingKey!
            let signature = try P256.Signing.ECDSASignature(derRepresentation: sig)
            check(key.publicKey.isValidSignature(signature, for: Data(tbs)), "self-signed with its key")
            let fields = derElements(parts[0].2)
            check(fields.count == 8, "version, serial, algorithm, issuer, validity, subject, key, extensions")
            check(fields[3].1 == fields[5].1, "issuer is subject")
            let spki = derElements(fields[6].2)
            check(Array(spki[1].2.dropFirst()) == Array(key.publicKey.x963Representation), "the public key is the one that signs")
            let validity = derElements(fields[4].2)
            check(validity[0].0 == 0x17 && String(decoding: validity[0].2, as: UTF8.self) == "240101000000Z")
            check(validity[1].0 == 0x18 && String(decoding: validity[1].2, as: UTF8.self) == "21240101000000Z")
            let serial = derElements(fields[1].1)[0].2
            check(serial[0] & 0x80 == 0 && serial.count <= 17, "a positive serial")
        }
        test("two_identities_are_two_keys") {
            let a = try LinkIdentity.generate(), b = try LinkIdentity.generate()
            check(a.fingerprint != b.fingerprint && a.privateKey != b.privateKey)
        }
        test("der_lengths_and_integers_follow_x690") {
            check(DER.length(0x7F) == [0x7F])
            check(DER.length(0x80) == [0x81, 0x80])
            check(DER.length(300) == [0x82, 0x01, 0x2C])
            check(DER.integer([0x00, 0x00, 0x05]) == [0x02, 0x01, 0x05])
            check(DER.integer([0x80]) == [0x02, 0x02, 0x00, 0x80])
            check(DER.integer([]) == [0x02, 0x01, 0x00])
            check(DER.oid([1, 2, 840, 10045, 2, 1]) == [0x06, 0x07, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01])
        }
    }

    // MARK: tests.rs (the conversation, over real sockets on loopback)

    static func conversationTests() {
        test("a_paired_phone_gets_sessions_and_answers_the_request") {
            let rig = try Rig()
            guard let c = TestClient(port: rig.server.port) else { return check(false, "connect") }
            c.hello(token: Rig.token)
            let welcome = c.expect("welcome")
            check(welcome?["v"] as? Int == 1 && welcome?["desktop"] as? String == "Test Mac" && welcome?["os"] as? String == "macos")
            check(welcome?["caps"] == nil, "a phone that sent no caps gets none")
            check(c.expect("sessions") != nil)
            rig.hub.publish(sessions: [session("working")], approval: approval("req-1", "npm run build"), now: LinkProtocol.nowMs())
            let a = c.expect("approval")
            check(a?["command"] as? String == "npm run build" && a?["tool"] as? String == "Bash")
            c.send(["type": "decision", "fingerprint": a?["fingerprint"] as? String ?? "", "decision": "allow"])
            check(c.expect("approvalResolved")?["fingerprint"] as? String == a?["fingerprint"] as? String)
            check(eventually { rig.recorder.all.count == 1 }, "the decision reached the agent")
            check(rig.recorder.all.first.map { $0 == ("req-1", true) } == true)
            c.send(["type": "ping"])
            check(c.expect("pong") != nil)
        }
        test("a_wrong_token_is_refused_and_gets_nothing") {
            let rig = try Rig()
            rig.hub.publish(sessions: [session("working")], approval: approval("req-1", "ls"), now: LinkProtocol.nowMs())
            let c = TestClient(port: rig.server.port)!
            c.hello(token: String(repeating: "x", count: Rig.token.count))
            let lines = c.drain(1)
            check(types(lines) == ["error"], "\(types(lines))")
            check(lines.first?["code"] as? String == "auth")
            check(c.closed(1))
            check(rig.hub.clients == 0)
        }
        test("a_token_of_another_length_or_a_missing_one_is_refused_too") {
            let rig = try Rig()
            for msg: [String: Any] in [["type": "hello", "v": 1, "token": "short"], ["type": "hello", "v": 1],
                                       ["type": "hello", "v": 2, "token": Rig.token], ["type": "hello", "v": "1", "token": Rig.token],
                                       ["type": "hello", "v": true, "token": Rig.token], ["type": "hello", "v": 1, "token": 5]] {
                let c = TestClient(port: rig.server.port)!
                c.send(msg)
                check(c.expect("error")?["code"] as? String == "auth", "\(msg)")
            }
        }
        test("a_phone_that_does_not_say_hello_first_gets_nowhere") {
            let rig = try Rig()
            for first: [UInt8] in [Array("{\"type\":\"ping\"}\n".utf8), Array("not json\n".utf8), Array("[1,2]\n".utf8)] {
                let c = TestClient(port: rig.server.port)!
                c.sendRaw(first)
                check(c.drain(0.5).isEmpty)
                check(c.closed(1))
            }
        }
        test("silence_before_the_hello_closes_the_connection") {
            let rig = try Rig(helloTimeout: 0.3)
            let c = TestClient(port: rig.server.port)!
            check(c.closed(2), "closed after the hello timeout")
            check(eventually { rig.shared.admission.counts.total == 0 }, "and its place is given back")
        }
        test("silence_after_the_hello_closes_the_connection_too") {
            let rig = try Rig(idleTimeout: 0.4)
            let c = rig.paired()!
            check(c.closed(3))
            check(eventually { rig.hub.clients == 0 })
        }
        test("a_decision_for_another_request_changes_nothing") {
            let rig = try Rig()
            let c = rig.paired()!
            rig.hub.publish(sessions: [], approval: approval("req-1", "ls"), now: LinkProtocol.nowMs())
            _ = c.expect("approval")
            c.send(["type": "decision", "fingerprint": LinkProtocol.fingerprint(pillId: "integration_claude", sessionId: "s1", tool: "Bash", command: "rm -rf /", inputKey: "req-1"), "decision": "allow"])
            c.send(["type": "decision", "fingerprint": rig.fingerprint, "decision": "always"])
            c.send(["type": "decision", "fingerprint": 42, "decision": "allow"])
            c.send(["type": "ping"])
            check(c.expect("pong") != nil, "still talking")
            check(rig.recorder.all.isEmpty)
            check(rig.hub.pendingFingerprint != nil)
        }
        test("a_decision_that_comes_too_late_is_ignored") {
            let clock = Atomic<UInt64>(1_000_000)
            let rig = try Rig(clock: { clock.load(ordering: .sequentiallyConsistent) })
            let c = rig.paired()!
            rig.hub.publish(sessions: [], approval: approval("req-1", "ls"), now: 1_000_000)
            let a = c.expect("approval")
            clock.store(1_000_000 + LinkProtocol.approvalTTLms + 1, ordering: .sequentiallyConsistent)
            c.send(["type": "decision", "fingerprint": a?["fingerprint"] as? String ?? "", "decision": "allow"])
            check(c.expect("approvalResolved") != nil, "the late request is dismissed")
            check(rig.recorder.all.isEmpty, "and not applied")
        }
        test("a_decision_after_the_desktop_answered_is_ignored") {
            let rig = try Rig()
            let c = rig.paired()!
            rig.hub.publish(sessions: [], approval: approval("req-2", "ls"), now: LinkProtocol.nowMs())
            let a = c.expect("approval")
            rig.hub.publish(sessions: [], approval: nil, now: LinkProtocol.nowMs())
            check(c.expect("approvalResolved") != nil)
            c.send(["type": "decision", "fingerprint": a?["fingerprint"] as? String ?? "", "decision": "allow"])
            c.send(["type": "ping"])
            _ = c.expect("pong")
            check(rig.recorder.all.isEmpty)
        }
        test("a_line_over_64_kib_closes_the_connection") {
            let rig = try Rig()
            let c = rig.paired()!
            _ = c.drain(0.2)
            c.sendRaw(Array(repeating: UInt8(ascii: "x"), count: LinkProtocol.maxLine + 1) + [10])
            let e = c.expect("error")
            check(e?["code"] as? String == "protocol")
            check(c.closed(2))
            check(eventually { rig.hub.clients == 0 })
        }
        test("an_unterminated_flood_closes_the_connection_too") {
            let rig = try Rig()
            let c = rig.paired()!
            c.sendRaw(Array(repeating: UInt8(ascii: "x"), count: LinkProtocol.maxLine + 10))
            check(c.expect("error")?["code"] as? String == "protocol")
        }
        test("a_line_of_exactly_64_kib_is_accepted") {
            let rig = try Rig()
            let c = rig.paired()!
            let head = Array("{\"type\":\"ping\",\"pad\":\"".utf8), tail = Array("\"}".utf8)
            let pad = [UInt8](repeating: UInt8(ascii: "a"), count: LinkProtocol.maxLine - head.count - tail.count)
            c.sendRaw(head + pad + tail + [13, 10])
            check(c.expect("pong") != nil)
        }
        test("pairing_again_locks_the_old_phone_out") {
            let rig = try Rig()
            let c = rig.paired()!
            rig.shared.replaceToken("brand-new-token-0123456789")
            rig.hub.kickAll(code: "auth", message: "unpaired")
            check(c.expect("error")?["code"] as? String == "auth")
            check(c.closed(2))
            let old = TestClient(port: rig.server.port)!
            old.hello(token: Rig.token)
            check(old.expect("error")?["code"] as? String == "auth", "the old token no longer works")
            let fresh = TestClient(port: rig.server.port)!
            fresh.hello(token: "brand-new-token-0123456789")
            check(fresh.expect("welcome") != nil, "the new one does")
        }
        test("turning_it_off_closes_the_phones_and_stops_listening") {
            let rig = try Rig()
            let c = rig.paired()!
            rig.server.stop()
            check(c.expect("error")?["code"] as? String == "closed")
            check(c.closed(2))
            check(TestClient(port: rig.server.port) == nil, "nothing listens any more")
        }
        test("bye_ends_the_conversation") {
            let rig = try Rig()
            let c = rig.paired()!
            c.send(["type": "bye"])
            check(c.closed(2))
            check(eventually { rig.hub.clients == 0 && rig.shared.admission.counts.total == 0 })
        }
        test("unknown_types_are_ignored") {
            let rig = try Rig()
            let c = rig.paired()!
            c.send(["type": "fromTheFuture", "x": [1, 2, 3]])
            c.send(["no type": true])
            c.send(["type": "answer", "fingerprint": "x", "picks": [["a"]]])   // no `answers` capability yet
            c.send(["type": "ping"])
            check(c.expect("pong") != nil)
        }
        test("over_the_wire_a_10000_character_command_is_flagged_and_allow_does_nothing") {
            let rig = try Rig()
            let c = rig.paired()!
            rig.hub.publish(sessions: [], approval: approval("req-long", String(repeating: "x", count: 10_000)), now: LinkProtocol.nowMs())
            let a = c.expect("approval")
            check(a?["truncated"] as? Bool == true && (a?["command"] as? String)?.count == 500)
            c.send(["type": "decision", "fingerprint": a?["fingerprint"] as? String ?? "", "decision": "allow"])
            c.send(["type": "ping"])
            _ = c.expect("pong")
            check(rig.recorder.all.isEmpty, "Allow for a cut command is never applied")
            c.send(["type": "decision", "fingerprint": a?["fingerprint"] as? String ?? "", "decision": "deny"])
            check(eventually { rig.recorder.all.count == 1 && rig.recorder.all[0] == ("req-long", false) })
        }
        test("the_total_number_of_connections_is_limited") {
            guard loopbackAliases() else { return print("skipped \(current): no loopback aliases here") }
            let rig = try Rig()
            var phones: [TestClient] = []
            for n in 0..<Admission.maxConnections {
                // Each from its own loopback address so the per-address and stranger limits do not get in the way.
                guard let c = TestClient(port: rig.server.port, source: "127.0.1.\(n + 1)") else { return check(false, "connect") }
                c.hello(token: Rig.token)
                check(c.expect("welcome") != nil, "phone \(n)")
                phones.append(c)
            }
            let ninth = TestClient(port: rig.server.port, source: "127.0.1.100")!
            ninth.hello(token: Rig.token)
            check(ninth.expect("welcome", 1) == nil && ninth.closed(1), "the ninth is not served")
        }
        test("one_address_cannot_hold_more_than_two_silent_connections") {
            guard loopbackAliases() else { return print("skipped \(current): no loopback aliases here") }
            let rig = try Rig()
            let a = TestClient(port: rig.server.port, source: "127.0.2.1")!
            let b = TestClient(port: rig.server.port, source: "127.0.2.1")!
            check(eventually { rig.shared.admission.counts.total == 2 })
            let third = TestClient(port: rig.server.port, source: "127.0.2.1")!
            third.hello(token: Rig.token)
            check(third.closed(1), "a third silent one from the same address is closed unserved")
            let other = TestClient(port: rig.server.port, source: "127.0.2.2")!
            other.hello(token: Rig.token)
            check(other.expect("welcome") != nil, "another address still gets in")
            _ = (a, b)
        }
        test("a_flood_from_several_addresses_still_leaves_room_for_the_known_phone") {
            guard loopbackAliases() else { return print("skipped \(current): no loopback aliases here") }
            let rig = try Rig()
            let phone = TestClient(port: rig.server.port, source: "127.0.3.1")!
            phone.hello(token: Rig.token)
            check(phone.expect("welcome") != nil)
            var flood: [TestClient] = []
            for n in 0..<10 { if let c = TestClient(port: rig.server.port, source: "127.0.4.\(n + 1)") { flood.append(c) } }
            check(eventually { rig.shared.admission.counts.strangers == Admission.maxUnauthStrangers })
            let again = TestClient(port: rig.server.port, source: "127.0.3.1")!
            again.hello(token: Rig.token)
            check(again.expect("welcome") != nil, "the paired phone gets in during the flood")
        }
    }
}
