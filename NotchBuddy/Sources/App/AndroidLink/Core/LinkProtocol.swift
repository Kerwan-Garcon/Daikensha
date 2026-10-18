#if ANDROID_LINK
import Foundation
#if canImport(Glibc)
import Glibc
#elseif canImport(Darwin)
import Darwin
#endif

// MARK: - Android link: protocol v1 constants and small rules
//
// docs/ANDROID_LINK.md is the specification; windows/src-tauri/src/phone_link/ is the reference
// implementation this port follows rule for rule. Everything in Core/ is pure (no AppKit, no
// Network.framework, no Keychain) so it also builds and is tested on Linux, in Docker.

enum LinkProtocol {
    static let version = 1
    static let defaultPort: UInt16 = 47821
    /// A line over this is a protocol error.
    static let maxLine = 64 * 1024
    /// The computer dismisses a request after this long (the phone offers it for 120 s).
    static let approvalTTLms: UInt64 = 115_000
    static let helloTimeout: TimeInterval = 10
    /// An authenticated phone may be silent this long: a sleeping phone checks in every few minutes only.
    static let idleTimeout: TimeInterval = 20 * 60
    /// Pause before answering a wrong token, so guessing is slow.
    static let authPenalty: TimeInterval = 0.4
    /// What a connection's writer may hold before the hub gives up on that phone.
    static let outQueueLimit = 64

    static let maxSessions = 16
    static let maxCommandChars = 500
    static let maxTextChars = 200
    static let maxAgentChars = 64
    static let maxToolChars = 64

    /// The session states the phone knows; anything else is sent as `idle`.
    static let states: Set<String> = [
        "idle", "working", "thinking", "searching", "approval", "question",
        "error", "finished", "ratelimit", "sleeping", "dizzy",
    ]

    /// Milliseconds since 1970, the protocol's clock.
    static func nowMs() -> UInt64 {
        UInt64(max(0, Date().timeIntervalSince1970 * 1000))
    }

    /// Length in Unicode scalars, the way the reference counts characters (Rust `chars()`).
    static func length(_ s: String) -> Int { s.unicodeScalars.count }

    /// The first `max` Unicode scalars (Rust `chars()`), so a fingerprint is the same on every computer.
    static func clip(_ s: String, _ max: Int) -> String {
        let scalars = s.unicodeScalars
        guard scalars.count > max else { return s }
        var out = String.UnicodeScalarView()
        out.append(contentsOf: scalars.prefix(max))
        return String(out)
    }

    /// The Mac's `ApprovalRelay.fingerprint` derivation: lowercase hex SHA-256 of the fields joined by
    /// U+001F. On the Android link `inputKey` is the request's id, so two requests for the same command
    /// still differ and a late decision for the first can never answer the second.
    static func fingerprint(pillId: String, sessionId: String, tool: String, command: String, inputKey: String) -> String {
        let joined = [pillId, sessionId, tool, command, inputKey].joined(separator: "\u{1F}")
        return LinkCrypto.sha256Hex(Array(joined.utf8))
    }

    /// One JSON object as a line (without its newline).
    static func line<T: Encodable>(_ value: T) -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.withoutEscapingSlashes]
        guard let data = try? encoder.encode(value) else { return "{}" }
        return String(decoding: data, as: UTF8.self)
    }
}

// MARK: - The local network

/// Who may connect: private, loopback and link-local addresses only (docs/ANDROID_LINK.md).
enum LinkAddress {
    static func isLocal(v4 b: [UInt8]) -> Bool {
        guard b.count == 4 else { return false }
        return b[0] == 10
            || (b[0] == 172 && (16...31).contains(b[1]))
            || (b[0] == 192 && b[1] == 168)
            || b[0] == 127
            || (b[0] == 169 && b[1] == 254)
    }

    static func isLocal(v6 b: [UInt8]) -> Bool {
        guard b.count == 16 else { return false }
        if let v4 = mappedV4(b) { return isLocal(v4: v4) }
        let loopback = b[0..<15].allSatisfy { $0 == 0 } && b[15] == 1
        return loopback
            || (b[0] & 0xFE) == 0xFC                     // fc00::/7
            || (b[0] == 0xFE && (b[1] & 0xC0) == 0x80)   // fe80::/10
    }

    /// `::ffff:a.b.c.d` → `a.b.c.d`.
    static func mappedV4(_ b: [UInt8]) -> [UInt8]? {
        guard b.count == 16, b[0..<10].allSatisfy({ $0 == 0 }), b[10] == 0xFF, b[11] == 0xFF else { return nil }
        return Array(b[12..<16])
    }

    /// The address as one key, whichever way it was written (a v4-mapped v6 address is its v4 address).
    static func key(v4 b: [UInt8]) -> String { b.map(String.init).joined(separator: ".") }

    static func key(v6 b: [UInt8]) -> String {
        if let v4 = mappedV4(b) { return key(v4: v4) }
        return LinkCrypto.hex(b)
    }

    /// Parses a textual address (tests, and the scope-free text Network.framework gives).
    static func bytes(_ text: String) -> (v4: [UInt8]?, v6: [UInt8]?) {
        let bare = text.split(separator: "%", maxSplits: 1).first.map(String.init) ?? text
        var a4 = in_addr()
        if inet_pton(AF_INET, bare, &a4) == 1 {
            return (withUnsafeBytes(of: &a4) { Array($0) }, nil)
        }
        var a6 = in6_addr()
        if inet_pton(AF_INET6, bare, &a6) == 1 {
            return (nil, withUnsafeBytes(of: &a6) { Array($0) })
        }
        return (nil, nil)
    }

    static func isLocal(_ text: String) -> Bool {
        let b = bytes(text)
        if let v4 = b.v4 { return isLocal(v4: v4) }
        if let v6 = b.v6 { return isLocal(v6: v6) }
        return false
    }

    static func key(_ text: String) -> String {
        let b = bytes(text)
        if let v4 = b.v4 { return key(v4: v4) }
        if let v6 = b.v6 { return key(v6: v6) }
        return text
    }
}
#endif
