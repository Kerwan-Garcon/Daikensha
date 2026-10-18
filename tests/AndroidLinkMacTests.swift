import Foundation
import Network
import Security
import Synchronization

// The Mac-only half of the Android link: LinkServer (Network.framework, TLS 1.2/1.3, Bonjour) with
// the identity the core makes, against a client that pins the certificate the way the phone does
// (Pairing.kt: the SHA-256 of the leaf, nothing else). The portable half is tested by
// scripts/test-android-link.sh, on Linux too. Run by scripts/test-android-link-mac.sh.
//
// The identity goes into a throwaway file keychain here: the app's data protection Keychain needs a
// signed app, which a CI runner does not have.

nonisolated(unsafe) var failures: [String] = []
nonisolated(unsafe) var passed = 0

func check(_ ok: Bool, _ what: String, line: Int = #line) {
    if ok { passed += 1 } else { failures.append("line \(line): \(what)") }
}

func eventually(_ seconds: Double = 5, _ cond: () -> Bool) -> Bool {
    let deadline = Date().addingTimeInterval(seconds)
    while Date() < deadline {
        if cond() { return true }
        RunLoop.current.run(until: Date().addingTimeInterval(0.02))
    }
    return cond()
}

final class Recorder: LinkHost {
    let decisions = Mutex<[(String, Bool)]>([])
    func decide(requestId: String, allow: Bool) { decisions.withLock { $0.append((requestId, allow)) } }
    var count: Int { decisions.withLock { $0.count } }
}

/// A phone: TLS with the certificate pinned by its SHA-256, newline-delimited JSON.
final class PinnedClient: @unchecked Sendable {
    let connection: NWConnection
    private let queue = DispatchQueue(label: "test.client")
    private let lines = Mutex<[[String: Any]]>([])
    private let buffer = Mutex<[UInt8]>([])
    let state = Mutex<String>("setup")

    init(port: UInt16, pin: String) {
        let tls = NWProtocolTLS.Options()
        sec_protocol_options_set_verify_block(tls.securityProtocolOptions, { _, trust, complete in
            let chain = SecTrustCopyCertificateChain(sec_trust_copy_ref(trust).takeRetainedValue()) as? [SecCertificate] ?? []
            let leaf = chain.first.map { SecCertificateCopyData($0) as Data } ?? Data()
            complete(LinkCrypto.sha256Hex(leaf) == pin)
        }, DispatchQueue(label: "test.verify"))
        connection = NWConnection(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!, using: NWParameters(tls: tls))
        connection.stateUpdateHandler = { [weak self] s in
            self?.state.withLock {
                switch s {
                case .ready: $0 = "ready"
                case .failed: $0 = "failed"
                case .cancelled: $0 = "cancelled"
                case .waiting: $0 = "waiting"
                default: break
                }
            }
        }
        connection.start(queue: queue)
        receive()
    }

    private func receive() {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 65536) { [self] data, _, done, error in
            if let data {
                buffer.withLock { b in
                    b += data
                    while let nl = b.firstIndex(of: 10) {
                        let line = Array(b[..<nl])
                        b.removeSubrange(...nl)
                        if let obj = (try? JSONSerialization.jsonObject(with: Data(line))) as? [String: Any] {
                            lines.withLock { $0.append(obj) }
                        }
                    }
                }
            }
            if done || error != nil {
                state.withLock { $0 = "closed" }
            } else {
                receive()
            }
        }
    }

    func send(_ object: [String: Any]) {
        let data = try! JSONSerialization.data(withJSONObject: object) + Data([10])
        connection.send(content: data, completion: .contentProcessed { _ in })
    }

    func expect(_ type: String, _ seconds: Double = 5) -> [String: Any]? {
        var found: [String: Any]?
        _ = eventually(seconds) {
            lines.withLock { l in
                if let i = l.firstIndex(where: { $0["type"] as? String == type }) {
                    found = l[i]
                    l.removeSubrange(...i)
                }
            }
            return found != nil
        }
        return found
    }

    var current: String { state.withLock { $0 } }
}

/// A file keychain of its own, with the core's key and certificate, paired into a SecIdentity.
func identityInThrowawayKeychain(_ id: LinkIdentity) -> SecIdentity? {
    let path = NSTemporaryDirectory() + "coucou-android-link-\(getpid()).keychain"
    var keychain: SecKeychain?
    let password = "test"
    guard SecKeychainCreate(path, UInt32(password.utf8.count), password, false, nil, &keychain) == errSecSuccess,
          let keychain else { return nil }
    let attributes: [String: Any] = [
        kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
        kSecAttrKeyClass as String: kSecAttrKeyClassPrivate,
        kSecAttrKeySizeInBits as String: 256,
    ]
    guard let key = SecKeyCreateWithData(id.privateKey as CFData, attributes as CFDictionary, nil),
          let certificate = SecCertificateCreateWithData(nil, Data(id.certificate) as CFData) else { return nil }
    let addKey = SecItemAdd([kSecClass as String: kSecClassKey, kSecValueRef as String: key,
                             kSecUseKeychain as String: keychain, kSecAttrLabel as String: "test"] as CFDictionary, nil)
    let addCert = SecItemAdd([kSecClass as String: kSecClassCertificate, kSecValueRef as String: certificate,
                              kSecUseKeychain as String: keychain] as CFDictionary, nil)
    guard addKey == errSecSuccess, addCert == errSecSuccess else {
        print("keychain: key \(addKey), certificate \(addCert)")
        return nil
    }
    var identity: SecIdentity?
    guard SecIdentityCreateWithCertificate(keychain, certificate, &identity) == errSecSuccess else { return nil }
    return identity
}

@main
enum AndroidLinkMacTests {
    static func main() {
        let id = try! LinkIdentity.generate()
        // Security reads the hand-made DER as a certificate, with the same key.
        let certificate = SecCertificateCreateWithData(nil, Data(id.certificate) as CFData)
        check(certificate != nil, "Security parses the certificate")
        if let certificate, let key = SecCertificateCopyKey(certificate),
           let raw = SecKeyCopyExternalRepresentation(key, nil) as Data? {
            check(raw == id.signingKey!.publicKey.x963Representation, "the certificate carries the key that signs")
        } else {
            check(false, "the certificate's key is readable")
        }
        check((SecCertificateCopySubjectSummary(certificate!) as String?) == "Coucou phone link", "its name")

        guard let identity = identityInThrowawayKeychain(id) else {
            print("FAIL could not make a SecIdentity")
            exit(1)
        }
        let recorder = Recorder()
        let hub = LinkHub(host: recorder)
        let token = "mac-test-token-0123456789"
        let shared = LinkShared(hub: hub, token: token, name: "Test Mac", authPenalty: 0.05)
        let server = LinkServer(shared: shared, identity: identity, fingerprint: id.fingerprint)
        server.start()
        check(eventually { server.port != nil }, "it listens")
        guard let port = server.port else { report(); return }
        check(port == LinkProtocol.defaultPort || port > 0, "on 47821 or a free port")

        // A phone that pins the right certificate gets in, sees the request, and its decision applies once.
        let phone = PinnedClient(port: port, pin: id.fingerprint)
        check(eventually { phone.current == "ready" }, "TLS handshake with the pinned certificate")
        phone.send(["type": "hello", "v": 1, "token": token, "device": "Test"])
        let welcome = phone.expect("welcome")
        check(welcome?["os"] as? String == "macos" && welcome?["desktop"] as? String == "Test Mac", "welcome")
        check(phone.expect("sessions") != nil, "sessions")
        hub.publish(sessions: [SessionIn(pillId: "integration_claude", agent: "Claude Code", state: "approval")],
                    approval: ApprovalIn(requestId: "req-1", sessionId: "s", pillId: "integration_claude", tool: "Bash", command: "ls"),
                    now: LinkProtocol.nowMs())
        let approval = phone.expect("approval")
        check(approval?["command"] as? String == "ls", "approval")
        phone.send(["type": "decision", "fingerprint": approval?["fingerprint"] as? String ?? "", "decision": "allow"])
        check(phone.expect("approvalResolved") != nil, "resolved")
        check(eventually { recorder.count == 1 }, "the decision reached the host")
        phone.send(["type": "ping"])
        check(phone.expect("pong") != nil, "pong")

        // A phone that expects another certificate never gets a TLS session, so it can send nothing.
        let other = PinnedClient(port: port, pin: String(repeating: "0", count: 64))
        check(eventually { other.current == "failed" || other.current == "cancelled" || other.current == "waiting" }, "a wrong pin fails the handshake")
        other.connection.cancel()

        // A wrong token: error auth, then closed.
        let stranger = PinnedClient(port: port, pin: id.fingerprint)
        _ = eventually { stranger.current == "ready" }
        stranger.send(["type": "hello", "v": 1, "token": String(repeating: "x", count: token.count)])
        check(stranger.expect("error")?["code"] as? String == "auth", "a wrong token is refused")
        check(eventually { stranger.current == "closed" || stranger.current == "cancelled" }, "and closed")

        // Turning it off tells the phone and stops listening.
        server.stop()
        check(phone.expect("error")?["code"] as? String == "closed", "the phone is told")
        check(eventually { server.port == nil }, "the port is released")
        report()
    }

    static func report() {
        if failures.isEmpty {
            print("android link (mac): \(passed) checks passed")
        } else {
            for f in failures { print("FAIL \(f)") }
            exit(1)
        }
    }
}
