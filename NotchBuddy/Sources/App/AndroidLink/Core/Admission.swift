#if ANDROID_LINK
import Foundation
import Synchronization

// MARK: - Android link: who may open a connection, and for how long it may stay unauthenticated
//
// docs/ANDROID_LINK.md, "Admission"; a line-for-line port of admission.rs. The server answers the
// local network before it knows who is calling, so a caller that has not said a valid hello is held to:
//   * at most 2 such connections per source address;
//   * a pool of 4 shared by all addresses that never authenticated, so at least half of the 8 slots
//     stay free for a phone this computer already knows;
//   * an address that authenticated is remembered (in memory only, 8 addresses) and is not held to that pool;
//   * when the pool is getting full the TLS handshake and the hello get 3 s instead of 10 s.
// Pure bookkeeping, no sockets and no clock: every rule is a unit test.

final class Admission: Sendable {
    static let maxConnections = 8
    static let maxUnauthPerAddress = 2
    static let maxUnauthStrangers = 4
    static let knownAddresses = 8
    static let patient: TimeInterval = 10
    static let hurried: TimeInterval = 3

    fileprivate struct State {
        var total = 0
        var strangersUnauth = 0
        var unauth: [String: Int] = [:]
        var known: [String] = []   // most recent first
    }

    fileprivate let state = Mutex(State())

    /// A slot for this caller (an address key from `LinkAddress.key`), or nil: the connection is closed unserved.
    /// The slot is given back when the ticket is released or deallocated, however the connection ends.
    func admit(_ address: String) -> Ticket? {
        state.withLock { s -> Ticket? in
            if s.total >= Self.maxConnections { return nil }
            if s.unauth[address, default: 0] >= Self.maxUnauthPerAddress { return nil }
            let stranger = !s.known.contains(address)
            if stranger && s.strangersUnauth >= Self.maxUnauthStrangers { return nil }
            // Looked at before this caller is counted: "is the pool getting full already?"
            let hurried = s.total + 2 >= Self.maxConnections || s.strangersUnauth * 2 >= Self.maxUnauthStrangers
            s.total += 1
            s.unauth[address, default: 0] += 1
            if stranger { s.strangersUnauth += 1 }
            return Ticket(admission: self, address: address, stranger: stranger, wait: hurried ? Self.hurried : Self.patient)
        }
    }

    /// (all connections, unauthenticated strangers), for the tests.
    var counts: (total: Int, strangers: Int) {
        state.withLock { ($0.total, $0.strangersUnauth) }
    }

    func knows(_ address: String) -> Bool {
        state.withLock { $0.known.contains(address) }
    }

    fileprivate static func releaseUnauth(_ s: inout State, _ address: String, _ stranger: Bool) {
        if let n = s.unauth[address] {
            s.unauth[address] = n > 1 ? n - 1 : nil
        }
        if stranger { s.strangersUnauth -= 1 }
    }
}

/// One accepted connection's slot.
final class Ticket: Sendable {
    private let admission: Admission
    let address: String
    private let stranger: Bool
    /// How long the TLS handshake, and then the hello, may each take (shorter when the pool is nearly full).
    let wait: TimeInterval

    private struct Flags {
        var unauthenticated = true
        var released = false
    }
    private let flags = Mutex(Flags())

    fileprivate init(admission: Admission, address: String, stranger: Bool, wait: TimeInterval) {
        self.admission = admission
        self.address = address
        self.stranger = stranger
        self.wait = wait
    }

    /// The caller said a valid hello: it no longer counts against the unauthenticated limits and its address is
    /// remembered, so a flood from elsewhere cannot keep it out.
    func authenticated() {
        let first = flags.withLock { f -> Bool in
            guard f.unauthenticated, !f.released else { return false }
            f.unauthenticated = false
            return true
        }
        guard first else { return }
        admission.state.withLock { s in
            Admission.releaseUnauth(&s, address, stranger)
            s.known.removeAll { $0 == address }
            s.known.insert(address, at: 0)
            if s.known.count > Admission.knownAddresses { s.known.removeLast(s.known.count - Admission.knownAddresses) }
        }
    }

    /// Gives the slot back. Safe to call more than once; deinit calls it too.
    func release() {
        let wasUnauth = flags.withLock { f -> Bool? in
            guard !f.released else { return nil }
            f.released = true
            return f.unauthenticated
        }
        guard let wasUnauth else { return }
        admission.state.withLock { s in
            s.total -= 1
            if wasUnauth { Admission.releaseUnauth(&s, address, stranger) }
        }
    }

    deinit { release() }
}
#endif
