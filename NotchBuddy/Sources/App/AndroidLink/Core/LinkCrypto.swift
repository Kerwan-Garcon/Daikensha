#if ANDROID_LINK
import Foundation
#if canImport(CryptoKit)
import CryptoKit
#else
import Crypto   // swift-crypto, same API: only the Linux test container uses it, the app never does
#endif
#if canImport(Security)
import Security
#endif

// MARK: - Android link: hashing, randomness, comparison
//
// The small primitives every part of the link uses. CryptoKit on the Mac; the portable core
// also builds on Linux (tests in Docker) against swift-crypto, which has the same API.

enum LinkCrypto {
    /// `count` bytes from the system's secure random source, nil if it failed.
    static func randomBytes(_ count: Int) -> [UInt8]? {
        var bytes = [UInt8](repeating: 0, count: count)
        #if canImport(Security)
        guard SecRandomCopyBytes(kSecRandomDefault, count, &bytes) == errSecSuccess else { return nil }
        #else
        var generator = SystemRandomNumberGenerator()   // getrandom(2) on Linux
        for i in bytes.indices { bytes[i] = generator.next() }
        #endif
        return bytes
    }

    static func sha256(_ bytes: some DataProtocol) -> [UInt8] {
        Array(SHA256.hash(data: Data(bytes)))
    }

    /// Lowercase hex SHA-256.
    static func sha256Hex(_ bytes: some DataProtocol) -> String {
        hex(sha256(bytes))
    }

    static func hex(_ bytes: some Sequence<UInt8>) -> String {
        let digits = Array("0123456789abcdef".utf8)
        var out = [UInt8]()
        for b in bytes {
            out.append(digits[Int(b >> 4)])
            out.append(digits[Int(b & 15)])
        }
        return String(decoding: out, as: UTF8.self)
    }

    /// Constant-time equality, so a token or fingerprint cannot be guessed byte by byte.
    /// The length is not secret (tokens and fingerprints have a fixed size).
    static func constantTimeEqual(_ a: some Collection<UInt8>, _ b: some Collection<UInt8>) -> Bool {
        guard a.count == b.count else { return false }
        var diff: UInt8 = 0
        for (x, y) in zip(a, b) { diff |= x ^ y }
        return diff == 0
    }
}
#endif
