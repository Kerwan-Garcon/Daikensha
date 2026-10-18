#if ANDROID_LINK
import Foundation
#if canImport(CryptoKit)
import CryptoKit
#else
import Crypto
#endif

// MARK: - Android link: the self-signed TLS certificate, built by hand
//
// The phone does not check a host name or an authority: it accepts exactly the certificate whose
// SHA-256 it was given in the pairing link (Pairing.kt). So the certificate only has to be a valid
// X.509 v3 that a TLS stack can read, carrying the P-256 key that signs the handshake. It is made
// once and kept in the Keychain. The fields are the ones rcgen writes for the Windows/Linux app
// (pairing.rs): ECDSA P-256 with SHA-256, CN "Coucou phone link", valid 2024-01-01 to 2124-01-01.
//
// A minimal DER encoder (RFC 5280, X.690) instead of a package: no dependency for a few dozen bytes
// written once. Checked by the tests with the system's own parsers (Security on the Mac, openssl in
// Docker), a real TLS handshake, and the Android client itself.

enum DER {
    static func tlv(_ tag: UInt8, _ content: [UInt8]) -> [UInt8] {
        [tag] + length(content.count) + content
    }

    static func length(_ n: Int) -> [UInt8] {
        if n < 0x80 { return [UInt8(n)] }
        var bytes: [UInt8] = []
        var v = n
        while v > 0 {
            bytes.insert(UInt8(v & 0xFF), at: 0)
            v >>= 8
        }
        return [0x80 | UInt8(bytes.count)] + bytes
    }

    static func sequence(_ parts: [UInt8]...) -> [UInt8] { tlv(0x30, parts.flatMap { $0 }) }
    static func set(_ parts: [UInt8]...) -> [UInt8] { tlv(0x31, parts.flatMap { $0 }) }

    /// A non-negative INTEGER from big-endian bytes (leading zeros dropped, a zero added if the top bit is set).
    static func integer(_ bigEndian: [UInt8]) -> [UInt8] {
        var b = Array(bigEndian.drop { $0 == 0 })
        if b.isEmpty { b = [0] }
        if b[0] & 0x80 != 0 { b.insert(0, at: 0) }
        return tlv(0x02, b)
    }

    static func oid(_ arcs: [UInt]) -> [UInt8] {
        precondition(arcs.count >= 2)
        var body: [UInt8] = [UInt8(arcs[0] * 40 + arcs[1])]
        for arc in arcs.dropFirst(2) {
            var chunk: [UInt8] = [UInt8(arc & 0x7F)]
            var v = arc >> 7
            while v > 0 {
                chunk.insert(UInt8(v & 0x7F) | 0x80, at: 0)
                v >>= 7
            }
            body += chunk
        }
        return tlv(0x06, body)
    }

    static func utf8String(_ s: String) -> [UInt8] { tlv(0x0C, Array(s.utf8)) }
    static func bitString(_ bytes: [UInt8]) -> [UInt8] { tlv(0x03, [0] + bytes) }
    static func explicit(_ n: UInt8, _ content: [UInt8]) -> [UInt8] { tlv(0xA0 | n, content) }

    /// UTCTime for years 1950 to 2049, GeneralizedTime after (RFC 5280 section 4.1.2.5). Midnight UTC.
    static func time(year: Int, month: Int, day: Int) -> [UInt8] {
        func two(_ v: Int) -> String { v < 10 ? "0\(v)" : "\(v)" }
        if year < 2050 {
            return tlv(0x17, Array("\(two(year % 100))\(two(month))\(two(day))000000Z".utf8))
        }
        return tlv(0x18, Array("\(year)\(two(month))\(two(day))000000Z".utf8))
    }
}

enum LinkCertificate {
    static let commonName = "Coucou phone link"

    // 1.2.840.10045.4.3.2 ecdsa-with-SHA256, 1.2.840.10045.2.1 id-ecPublicKey, 1.2.840.10045.3.1.7 prime256v1,
    // 2.5.4.3 commonName, 2.5.29.19 basicConstraints.
    private static let ecdsaWithSHA256 = DER.oid([1, 2, 840, 10045, 4, 3, 2])
    private static let ecPublicKey = DER.oid([1, 2, 840, 10045, 2, 1])
    private static let prime256v1 = DER.oid([1, 2, 840, 10045, 3, 1, 7])
    private static let cnOID = DER.oid([2, 5, 4, 3])
    private static let basicConstraintsOID = DER.oid([2, 5, 29, 19])

    /// A self-signed certificate for this key, as DER. `serial` is 16 random bytes.
    static func make(key: P256.Signing.PrivateKey, serial: [UInt8]) throws -> [UInt8] {
        let algorithm = DER.sequence(ecdsaWithSHA256)
        let name = DER.sequence(DER.set(DER.sequence(cnOID, DER.utf8String(commonName))))
        let validity = DER.sequence(DER.time(year: 2024, month: 1, day: 1), DER.time(year: 2124, month: 1, day: 1))
        let spki = DER.sequence(DER.sequence(ecPublicKey, prime256v1),
                                DER.bitString(Array(key.publicKey.x963Representation)))
        // basicConstraints, critical, CA:FALSE (an empty SEQUENCE): this certificate signs nothing else.
        let basicConstraints = DER.sequence(basicConstraintsOID, DER.tlv(0x01, [0xFF]), DER.tlv(0x04, DER.sequence()))
        let extensions = DER.explicit(3, DER.sequence(basicConstraints))
        let tbs = DER.sequence(
            DER.explicit(0, DER.integer([2])),   // v3
            DER.integer(serial),
            algorithm,
            name,
            validity,
            name,
            spki,
            extensions)
        let signature = try key.signature(for: Data(tbs)).derRepresentation
        return DER.sequence(tbs, algorithm, DER.bitString(Array(signature)))
    }

    /// `-----BEGIN CERTIFICATE-----` text, for tools that want PEM (the Linux test harness).
    static func pem(_ der: [UInt8]) -> String {
        let b64 = Data(der).base64EncodedString()
        var lines: [String] = []
        var rest = Substring(b64)
        while !rest.isEmpty {
            lines.append(String(rest.prefix(64)))
            rest = rest.dropFirst(64)
        }
        return "-----BEGIN CERTIFICATE-----\n" + lines.joined(separator: "\n") + "\n-----END CERTIFICATE-----\n"
    }
}

/// The certificate and its key, as the TLS server needs them.
struct LinkIdentity: Sendable {
    let certificate: [UInt8]
    /// The P-256 private key, raw (x963), only ever held in memory and in the Keychain.
    let privateKey: Data
    /// Lowercase hex SHA-256 of the certificate: the `fp` of the pairing link.
    var fingerprint: String { LinkCrypto.sha256Hex(certificate) }

    var signingKey: P256.Signing.PrivateKey? { try? P256.Signing.PrivateKey(x963Representation: privateKey) }

    /// A new key and its certificate.
    static func generate() throws -> LinkIdentity {
        guard let serial = LinkCrypto.randomBytes(16) else { throw LinkError.noRandom }
        var s = serial
        s[0] &= 0x7F   // a positive serial of 16 bytes at most (RFC 5280 allows 20)
        if s[0] == 0 { s[0] = 1 }
        let key = P256.Signing.PrivateKey()
        return LinkIdentity(certificate: try LinkCertificate.make(key: key, serial: s), privateKey: key.x963Representation)
    }
}

enum LinkError: Error, Equatable {
    case noRandom
    case keychain(Int32)
    case noIdentity
    case cannotListen(String)
}
#endif
