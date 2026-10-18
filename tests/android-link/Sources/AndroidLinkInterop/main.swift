import Foundation
import Synchronization
#if canImport(Glibc)
import Glibc
#elseif canImport(Darwin)
import Darwin
#endif

// The Mac's Android link core, run for the Android app's tests: the Swift twin of
// windows/src-tauri/src/phone_link/interop.rs, with the same protocol on stdin and stdout, so the
// Kotlin `RustDesktopInteropTest` drives it unchanged (docker/android-link/cargo-shim.sh starts it).
//
// It prints one JSON line (the port, the certificate fingerprint, the pairing link), then obeys one
// command per line and answers `OK <command>`:
//
//   sessions                 publish two sample sessions
//   approval <id> <command>  a permission request starts waiting
//   clear                    the request was answered at the desk
//   repair <token>           pair again with a new token (phones are kicked)
//   quit
//
//   DECISION allow|deny <id> printed when a phone's decision is applied
//
// The core speaks plain TCP here; socat puts TLS in front of it with the certificate and key the core
// made (LinkIdentity), so what the phone pins and checks is exactly what the Mac would serve.

func say(_ text: String) {
    // One write per line, so lines printed from two threads never mix.
    FileHandle.standardOutput.write(Data((text + "\n").utf8))
}

struct Printing: LinkHost {
    func decide(requestId: String, allow: Bool) {
        say("DECISION \(allow ? "allow" : "deny") \(requestId)")
    }
}

func freePort() -> UInt16 {
    #if canImport(Glibc)
    let fd = socket(AF_INET, Int32(SOCK_STREAM.rawValue), 0)
    #else
    let fd = socket(AF_INET, SOCK_STREAM, 0)
    #endif
    var addr = sockaddr_in()
    #if canImport(Darwin)
    addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
    #endif
    addr.sin_family = sa_family_t(AF_INET)
    inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr)
    _ = withUnsafePointer(to: &addr) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
    }
    var got = sockaddr_in()
    var len = socklen_t(MemoryLayout<sockaddr_in>.size)
    _ = withUnsafeMutablePointer(to: &got) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(fd, $0, &len) }
    }
    Glibc_close(fd)
    return UInt16(bigEndian: got.sin_port)
}

/// Waits until something accepts TCP connections on this port.
func waitForPort(_ port: UInt16, seconds: Double = 10) -> Bool {
    let deadline = Date().addingTimeInterval(seconds)
    while Date() < deadline {
        if TestClient(port: port) != nil { return true }
        usleep(50_000)
    }
    return false
}

let arguments = CommandLine.arguments
func option(_ name: String) -> String? {
    guard let i = arguments.firstIndex(of: name), i + 1 < arguments.count else { return nil }
    return arguments[i + 1]
}

let name = option("--name") ?? "Swift desktop"
let token = ProcessInfo.processInfo.environment["COUCOU_INTEROP_TOKEN"] ?? "interop-token-0123456789abcdef"
let identity = try LinkPairing.loadOrCreateIdentity(MemoryIdentityStore())
let hub = LinkHub(host: Printing())
let shared = LinkShared(hub: hub, token: token, name: name)
let server = try PosixServer(shared: shared)

// TLS in front: socat with the core's certificate and key.
let dir = FileManager.default.temporaryDirectory.appendingPathComponent("coucou-interop-\(getpid())")
try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
let certPath = dir.appendingPathComponent("cert.pem").path
let keyPath = dir.appendingPathComponent("key.pem").path
try LinkCertificate.pem(identity.certificate).write(toFile: certPath, atomically: true, encoding: .utf8)
try identity.signingKey!.pemRepresentation.write(toFile: keyPath, atomically: true, encoding: .utf8)
let tlsPort = freePort()
let socat = Process()
socat.executableURL = URL(fileURLWithPath: "/usr/bin/socat")
socat.arguments = [
    "OPENSSL-LISTEN:\(tlsPort),bind=127.0.0.1,reuseaddr,fork,verify=0,cert=\(certPath),key=\(keyPath)",
    "TCP:127.0.0.1:\(server.port)",
]
try socat.run()
guard waitForPort(tlsPort) else {
    say("socat did not start")
    exit(1)
}

let link = LinkPairing.link(host: "127.0.0.1", port: tlsPort, fingerprint: identity.fingerprint, token: token, name: name)
say("{\"listening\":\(tlsPort),\"fp\":\"\(identity.fingerprint)\",\"link\":\"\(link)\"}")

while let line = readLine() {
    let words = line.split(separator: " ", maxSplits: 2).map(String.init)
    switch (words.first, words.count) {
    case ("sessions", _):
        hub.publish(sessions: [
            SessionIn(pillId: "integration_claude", agent: "Claude Code", state: "working", statusText: "Editing files", stepIndex: 2, stepCount: 6),
            SessionIn(pillId: "agent_gemini", agent: "Gemini CLI", state: "sleeping"),
        ], approval: nil, now: LinkProtocol.nowMs())
    case ("approval", 3):
        hub.publish(sessions: [], approval: ApprovalIn(requestId: words[1], sessionId: "interop", pillId: "integration_claude",
                                                       tool: "Bash", command: words[2]), now: LinkProtocol.nowMs())
    case ("clear", _):
        hub.publish(sessions: [], approval: nil, now: LinkProtocol.nowMs())
    case ("repair", 2...):
        shared.replaceToken(words[1])
        hub.kickAll(code: "auth", message: "unpaired")
    case ("quit", _):
        socat.terminate()
        server.stop()
        try? FileManager.default.removeItem(at: dir)
        exit(0)
    default:
        say("? \(line)")
    }
    say("OK \(line)")
}
socat.terminate()
server.stop()
