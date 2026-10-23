#if ANDROID_LINK
import Foundation
#if canImport(Glibc)
import Glibc
#elseif canImport(Darwin)
import Darwin
#endif

// MARK: - Android link: pairing token, pairing link, and where the secrets live
//
// Port of pairing.rs. The token and the certificate's key live in the Keychain and nowhere else; if
// it cannot be read or written the link does not start: there is no fallback to disk.

/// Where the link's secrets are kept: the Keychain in the app, memory in the tests.
protocol LinkSecretStore: Sendable {
    func get(_ key: String) -> String?
    func set(_ key: String, _ value: String) throws
}

/// Where the TLS identity is kept. On the Mac the key and the certificate are Keychain items of their
/// own, so Network.framework can use them as a `SecIdentity`.
protocol LinkIdentityStore: Sendable {
    func load() -> LinkIdentity?
    func save(_ identity: LinkIdentity) throws
}

enum LinkPairing {
    static let tokenKey = "phone-link-token"

    /// The stored identity, else a new one, stored before it is used.
    static func loadOrCreateIdentity(_ store: LinkIdentityStore) throws -> LinkIdentity {
        // A key that no longer parses means a damaged entry: start over (the phone pairs again).
        if let id = store.load(), id.signingKey != nil, !id.certificate.isEmpty {
            return id
        }
        let id = try LinkIdentity.generate()
        try store.save(id)
        return id
    }

    /// The pairing token: the stored one, else a new one.
    static func token(_ store: LinkSecretStore) throws -> String {
        if let t = store.get(tokenKey), isToken(t) { return t }
        return try newToken(store)
    }

    /// A fresh token. Whatever phone held the old one is locked out.
    static func newToken(_ store: LinkSecretStore) throws -> String {
        let token = try randomToken()
        try store.set(tokenKey, token)
        return token
    }

    /// 16 to 128 characters of `[A-Za-z0-9_-]` (docs/ANDROID_LINK.md).
    static func isToken(_ s: String) -> Bool {
        (16...128).contains(s.utf8.count) && s.utf8.allSatisfy { b in
            (b >= 0x30 && b <= 0x39) || (b >= 0x41 && b <= 0x5A) || (b >= 0x61 && b <= 0x7A) || b == 0x5F || b == 0x2D
        }
    }

    /// 32 symbols of a 64-symbol alphabet from 32 random bytes: 192 bits.
    static func randomToken() throws -> String {
        let alphabet = Array("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_".utf8)
        guard let bytes = LinkCrypto.randomBytes(32) else { throw LinkError.noRandom }
        return String(decoding: bytes.map { alphabet[Int($0 & 63)] }, as: UTF8.self)
    }

    /// `coucou://pair?v=1&host=…&port=…&fp=…&token=…&name=…`
    static func link(host: String, port: UInt16, fingerprint: String, token: String, name: String) -> String {
        "coucou://pair?v=1&host=\(percentEncode(host))&port=\(port)&fp=\(fingerprint)&token=\(token)&name=\(percentEncode(name))"
    }

    /// Unreserved characters and `:` stay, every other byte of the UTF-8 is `%XX` (as pairing.rs does).
    static func percentEncode(_ s: String) -> String {
        var out = ""
        for b in s.utf8 {
            let keep = (b >= 0x30 && b <= 0x39) || (b >= 0x41 && b <= 0x5A) || (b >= 0x61 && b <= 0x7A)
                || b == 0x2D || b == 0x5F || b == 0x2E || b == 0x7E || b == 0x3A
            if keep {
                out.append(Character(Unicode.Scalar(b)))
            } else {
                let hex = Array("0123456789ABCDEF")
                out += "%\(hex[Int(b >> 4)])\(hex[Int(b & 15)])"
            }
        }
        return out
    }

    /// The name the phone shows: at most 48 characters, never empty.
    static func displayName(_ raw: String?) -> String {
        let trimmed = (raw ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "Coucou" : LinkProtocol.clip(trimmed, 48)
    }

    /// This computer's IPv4 address on the local network: the one the system would use to reach the
    /// outside. Connecting a UDP socket sends nothing; it only makes the system pick the interface.
    static func lanAddress() -> String? {
        #if canImport(Glibc)
        let fd = socket(AF_INET, Int32(SOCK_DGRAM.rawValue), 0)
        #else
        let fd = socket(AF_INET, SOCK_DGRAM, 0)
        #endif
        guard fd >= 0 else { return nil }
        defer { close(fd) }
        var remote = sockaddr_in()
        #if canImport(Darwin)
        remote.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        #endif
        remote.sin_family = sa_family_t(AF_INET)
        remote.sin_port = in_port_t(9).bigEndian
        inet_pton(AF_INET, "192.0.2.1", &remote.sin_addr)
        let connected = withUnsafePointer(to: &remote) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        guard connected == 0 else { return nil }
        var local = sockaddr_in()
        var len = socklen_t(MemoryLayout<sockaddr_in>.size)
        let got = withUnsafeMutablePointer(to: &local) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(fd, $0, &len) }
        }
        guard got == 0 else { return nil }
        let bytes = withUnsafeBytes(of: &local.sin_addr) { Array($0) }
        guard bytes != [0, 0, 0, 0], bytes[0] != 127 else { return nil }
        return LinkAddress.key(v4: bytes)
    }
}

// MARK: - The announcement on the local network (port of discovery.rs, what is said; LinkServer says it)

enum LinkDiscovery {
    /// The DNS-SD service type the phone browses for.
    static let serviceType = "_coucou._tcp"
    /// How many hex characters of the certificate fingerprint go into the TXT record.
    static let fingerprintIdLength = 16

    /// The short, non-secret id of a certificate fingerprint: its first 16 lowercase hex characters.
    static func fingerprintId(_ fingerprint: String) -> String? {
        let f = fingerprint.trimmingCharacters(in: .whitespaces).lowercased()
        guard f.utf8.count >= fingerprintIdLength, f.utf8.allSatisfy({ ($0 >= 0x30 && $0 <= 0x39) || ($0 >= 0x61 && $0 <= 0x66) }) else {
            return nil
        }
        return String(f.prefix(fingerprintIdLength))
    }

    /// The TXT record: the protocol version and the short id. Nothing else, ever.
    static func txt(_ fingerprint: String) -> [String: String]? {
        guard let id = fingerprintId(fingerprint) else { return nil }
        return ["v": "1", "fp": id]
    }

    /// The instance name: the computer's name, cut to fit one DNS label (63 bytes).
    static func instanceName(_ name: String) -> String {
        var out = ""
        for ch in name where !ch.isNewline {
            if (out + String(ch)).utf8.count > 63 { break }
            out.append(ch)
        }
        return out.isEmpty ? "Coucou" : out
    }
}
#endif
