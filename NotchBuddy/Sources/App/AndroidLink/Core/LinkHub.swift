#if ANDROID_LINK
import Foundation
import Synchronization

// MARK: - Android link: what the phones are told, and what they may decide
//
// A port of hub.rs. The hub holds the last list of sessions and the one permission request still
// waiting, as the Mac's AppState publishes them (LinkFeed). It knows nothing about sockets or
// AppKit: connections subscribe to it, and a decision goes back to the app through `LinkHost`.
//
// The rules that matter:
//   * a phone's decision is applied only if its fingerprint is the one of the request that is
//     *still* pending (same session, tool, command and request);
//   * only one decision is ever applied per request;
//   * a request older than 115 s is dismissed, whatever the phone says;
//   * `allow` for a command the phone was shown cut short is refused; `deny` works;
//   * nothing here can block the agent: the Mac's own card keeps working, a phone that never
//     answers changes nothing, and a phone that cannot keep up is dropped, not waited for.

/// What reaches a connection's writer.
enum LinkOut: Sendable, Equatable {
    /// One JSON line, without its newline.
    case line(String)
    /// Tell the phone why, then close.
    case close(code: String, message: String)
}

/// A connection's way out. `offer` never blocks: false means the phone cannot keep up (or is gone).
protocol LinkSink: AnyObject, Sendable {
    func offer(_ out: LinkOut) -> Bool
}

/// Applies a phone's decision as a click on the island would.
protocol LinkHost: Sendable {
    func decide(requestId: String, allow: Bool)
}

/// Set when the hub gives up on a phone, so its connection closes (and the phone reconnects to a
/// fresh picture) instead of staying open and silently going stale.
final class EvictionFlag: Sendable {
    private let value = Atomic<Bool>(false)
    var isSet: Bool { value.load(ordering: .sequentiallyConsistent) }
    func set() { value.store(true, ordering: .sequentiallyConsistent) }
}

/// One agent session, as the Mac reports it.
struct SessionIn: Sendable, Equatable {
    var pillId: String
    var agent: String = ""
    var state: String = "idle"
    var statusText: String = ""
    var stepIndex: Int = 0
    var stepCount: Int = 0
}

/// The permission request waiting on the island.
struct ApprovalIn: Sendable, Equatable {
    var requestId: String
    var sessionId: String
    var pillId: String
    var tool: String
    var command: String
}

/// What a connection was offered (docs/ANDROID_LINK.md, "Capabilities").
struct LinkCaps: Sendable, Equatable {
    var details = false
    var answers = false
    var prefs = false
    var diffs = false
    var usage = false
    var relay = false
    /// The services the user allowed for the phone (empty: the capability is not offered).
    var services: [String] = []
}

/// What became of a decision from a phone.
enum LinkOutcome: Equatable {
    case applied
    /// Not the pending request (another command, or already answered).
    case noMatch
    /// The request was pending but is older than the limit.
    case late
    /// Neither `allow` nor `deny`.
    case invalid
    /// `allow` for a command cut to fit the phone: refused, the request stays pending.
    case tooLong
}

final class LinkHub: Sendable {
    fileprivate struct Session: Equatable, Encodable {
        var pillId: String
        var agent: String
        var state: String
        var statusText: String
        var stepIndex: Int
        var stepCount: Int
        var updatedAt: UInt64

        func sameContent(_ other: Session) -> Bool {
            var a = self, b = other
            a.updatedAt = 0
            b.updatedAt = 0
            return a == b
        }
    }

    fileprivate struct Approval {
        var requestId: String
        var fingerprint: String
        var pillId: String
        var tool: String
        var command: String
        var truncated: Bool
        var createdAt: UInt64
    }

    fileprivate struct Sub {
        var id: UInt64
        var sink: LinkSink
        var evicted: EvictionFlag
        var caps: LinkCaps
    }

    fileprivate struct Inner {
        var sessions: [Session] = []
        var approval: Approval?
        var subs: [Sub] = []
        var nextSub: UInt64 = 0
    }

    private let host: LinkHost
    private let inner = Mutex(Inner())

    init(host: LinkHost) {
        self.host = host
    }

    /// The Mac's current picture. Sends the phones only what changed.
    func publish(sessions: [SessionIn], approval: ApprovalIn?, now: UInt64) {
        inner.withLock { inner in
            // Sessions: `updatedAt` moves only where something changed.
            let next: [Session] = sessions.filter { !$0.pillId.isEmpty }.prefix(LinkProtocol.maxSessions).map { s in
                var fresh = Session(
                    pillId: s.pillId,
                    agent: LinkProtocol.clip(s.agent, LinkProtocol.maxAgentChars),
                    state: LinkProtocol.states.contains(s.state) ? s.state : "idle",
                    statusText: LinkProtocol.clip(s.statusText, LinkProtocol.maxTextChars),
                    stepIndex: max(0, s.stepIndex),
                    stepCount: max(0, s.stepCount),
                    updatedAt: now)
                if let old = inner.sessions.first(where: { $0.pillId == fresh.pillId }), old.sameContent(fresh) {
                    fresh.updatedAt = old.updatedAt
                }
                return fresh
            }
            if next != inner.sessions {
                inner.sessions = next
                Self.broadcastSessions(&inner)
            }

            // The pending request: a new one replaces the old, none retracts it.
            let same: Bool
            switch (inner.approval, approval) {
            case let (a?, b?): same = a.requestId == b.requestId
            case (nil, nil): same = true
            default: same = false
            }
            if same { return }
            if let old = inner.approval {
                inner.approval = nil
                Self.broadcast(&inner, Self.resolvedLine(old.fingerprint))
            }
            if let a = approval, !a.requestId.isEmpty {
                let tool = LinkProtocol.clip(a.tool, LinkProtocol.maxToolChars)
                let truncated = LinkProtocol.length(a.command) > LinkProtocol.maxCommandChars
                let command = LinkProtocol.clip(a.command, LinkProtocol.maxCommandChars)
                let pending = Approval(
                    requestId: a.requestId,
                    fingerprint: LinkProtocol.fingerprint(pillId: a.pillId, sessionId: a.sessionId, tool: tool,
                                                          command: command, inputKey: a.requestId),
                    pillId: a.pillId, tool: tool, command: command, truncated: truncated, createdAt: now)
                inner.approval = pending
                Self.broadcast(&inner, Self.approvalLine(pending))
            }
        }
    }

    /// Registers a connection that has authenticated and brings it up to date: the sessions, then
    /// the request still waiting, if any.
    func subscribe(_ sink: LinkSink, now: UInt64, caps: LinkCaps = LinkCaps()) -> (id: UInt64, evicted: EvictionFlag) {
        inner.withLock { inner in
            Self.expire(&inner, now: now)
            let id = inner.nextSub
            inner.nextSub += 1
            _ = sink.offer(.line(Self.sessionsLine(inner.sessions)))
            if let a = inner.approval {
                _ = sink.offer(.line(Self.approvalLine(a)))
            }
            let evicted = EvictionFlag()
            inner.subs.append(Sub(id: id, sink: sink, evicted: evicted, caps: caps))
            return (id, evicted)
        }
    }

    func unsubscribe(_ id: UInt64) {
        inner.withLock { $0.subs.removeAll { $0.id == id } }
    }

    /// Unpaired or switched off: every connection is told and closed.
    func kickAll(code: String, message: String) {
        let subs = inner.withLock { inner -> [Sub] in
            defer { inner.subs = [] }
            return inner.subs
        }
        for s in subs { _ = s.sink.offer(.close(code: code, message: message)) }
    }

    var clients: Int { inner.withLock { $0.subs.count } }

    /// The fingerprint of the request still pending, for the tests.
    var pendingFingerprint: String? { inner.withLock { $0.approval?.fingerprint } }

    /// A phone's answer. Applied only for the request that is still pending.
    @discardableResult
    func decide(fingerprint: String, decision: String, now: UInt64) -> LinkOutcome {
        let allow: Bool
        switch decision {
        case "allow": allow = true
        case "deny": allow = false
        default: return .invalid
        }
        let result = inner.withLock { inner -> (LinkOutcome, String?) in
            guard let a = inner.approval,
                  LinkCrypto.constantTimeEqual(Array(a.fingerprint.utf8), Array(fingerprint.utf8)) else {
                return (.noMatch, nil)
            }
            // A command the phone could not show in full is never allowed from the phone.
            if allow && a.truncated { return (.tooLong, nil) }
            let late = now > a.createdAt && now - a.createdAt > LinkProtocol.approvalTTLms
            // Taken in every case: a request is answered once, or dismissed.
            inner.approval = nil
            Self.broadcast(&inner, Self.resolvedLine(a.fingerprint))
            return late ? (.late, nil) : (.applied, a.requestId)
        }
        // Outside the lock: the host answers the agent and touches the island.
        if let requestId = result.1 {
            host.decide(requestId: requestId, allow: allow)
        }
        return result.0
    }

    // MARK: Lines

    private struct SessionsLine: Encodable {
        let type = "sessions"
        let sessions: [Session]
    }

    private struct ApprovalLine: Encodable {
        let type = "approval"
        let pillId: String
        let fingerprint: String
        let tool: String
        let command: String
        let truncated: Bool
        let createdAt: UInt64
    }

    private struct ResolvedLine: Encodable {
        let type = "approvalResolved"
        let fingerprint: String
    }

    fileprivate static func sessionsLine(_ sessions: [Session]) -> String {
        LinkProtocol.line(SessionsLine(sessions: sessions))
    }

    fileprivate static func approvalLine(_ a: Approval) -> String {
        LinkProtocol.line(ApprovalLine(pillId: a.pillId, fingerprint: a.fingerprint, tool: a.tool,
                                       command: a.command, truncated: a.truncated, createdAt: a.createdAt))
    }

    fileprivate static func resolvedLine(_ fingerprint: String) -> String {
        LinkProtocol.line(ResolvedLine(fingerprint: fingerprint))
    }

    // MARK: Sending

    fileprivate static func expire(_ inner: inout Inner, now: UInt64) {
        if let a = inner.approval, now > a.createdAt, now - a.createdAt > LinkProtocol.approvalTTLms {
            inner.approval = nil
            broadcast(&inner, resolvedLine(a.fingerprint))
        }
    }

    /// Sends one line to every subscriber; one that cannot keep up is dropped rather than waited for.
    fileprivate static func broadcast(_ inner: inout Inner, _ line: String) {
        inner.subs.removeAll { s in
            let kept = s.sink.offer(.line(line))
            if !kept { s.evicted.set() }
            return !kept
        }
    }

    fileprivate static func broadcastSessions(_ inner: inout Inner) {
        broadcast(&inner, sessionsLine(inner.sessions))
    }
}
#endif
