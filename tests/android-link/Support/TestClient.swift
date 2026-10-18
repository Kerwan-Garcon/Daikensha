import Foundation
#if canImport(Glibc)
import Glibc
#elseif canImport(Darwin)
import Darwin
#endif

// A phone for the tests: a plain TCP client that writes lines and reads JSON lines back.

final class TestClient {
    let fd: Int32
    private var buffer: [UInt8] = []

    /// Connects to 127.0.0.1:port, from `source` if given (another loopback address, to look like another device).
    init?(port: UInt16, source: String? = nil) {
        #if canImport(Glibc)
        let fd = socket(AF_INET, Int32(SOCK_STREAM.rawValue), 0)
        #else
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        #endif
        guard fd >= 0 else { return nil }
        if let source {
            var local = sockaddr_in()
            #if canImport(Darwin)
            local.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
            #endif
            local.sin_family = sa_family_t(AF_INET)
            inet_pton(AF_INET, source, &local.sin_addr)
            let ok = withUnsafePointer(to: &local) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
            }
            guard ok == 0 else { Glibc_close(fd); return nil }
        }
        var addr = sockaddr_in()
        #if canImport(Darwin)
        addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        #endif
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = in_port_t(port).bigEndian
        inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr)
        let connected = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { connect(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
        }
        guard connected == 0 else { Glibc_close(fd); return nil }
        var tv = timeval(tv_sec: 0, tv_usec: 100_000)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, socklen_t(MemoryLayout<timeval>.size))
        self.fd = fd
    }

    deinit { Glibc_close(fd) }

    func sendRaw(_ bytes: [UInt8]) {
        var off = 0
        while off < bytes.count {
            let n = bytes.withUnsafeBytes { raw -> Int in
                #if canImport(Glibc)
                return Glibc.send(fd, raw.baseAddress! + off, raw.count - off, Int32(MSG_NOSIGNAL))
                #else
                return Darwin.send(fd, raw.baseAddress! + off, raw.count - off, 0)
                #endif
            }
            if n <= 0 { return }
            off += n
        }
    }

    func send(_ object: [String: Any]) {
        let data = try! JSONSerialization.data(withJSONObject: object)
        sendRaw(Array(data) + [10])
    }

    func hello(token: String, caps: [String]? = nil) {
        var msg: [String: Any] = ["type": "hello", "v": 1, "token": token, "device": "Test phone"]
        if let caps { msg["caps"] = caps }
        send(msg)
    }

    enum Read { case line([String: Any]), closed, timeout }

    /// The next JSON line within `seconds`.
    func read(_ seconds: Double = 3) -> Read {
        let deadline = Date().addingTimeInterval(seconds)
        while true {
            if let nl = buffer.firstIndex(of: 10) {
                let line = Array(buffer[..<nl])
                buffer.removeSubrange(...nl)
                if let obj = (try? JSONSerialization.jsonObject(with: Data(line))) as? [String: Any] { return .line(obj) }
                continue
            }
            if Date() > deadline { return .timeout }
            var chunk = [UInt8](repeating: 0, count: 8192)
            let n = chunk.withUnsafeMutableBytes { recv(fd, $0.baseAddress, $0.count, 0) }
            if n == 0 { return .closed }
            if n < 0 {
                if errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR { continue }
                return .closed
            }
            buffer += chunk[0..<n]
        }
    }

    /// The next line of this type, skipping others; nil if none came.
    func expect(_ type: String, _ seconds: Double = 3) -> [String: Any]? {
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline {
            switch read(deadline.timeIntervalSinceNow) {
            case .line(let l) where l["type"] as? String == type: return l
            case .line: continue
            case .closed, .timeout: return nil
            }
        }
        return nil
    }

    /// True if the server closed the connection within `seconds` (lines before that are skipped).
    func closed(_ seconds: Double = 3) -> Bool {
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline {
            switch read(deadline.timeIntervalSinceNow) {
            case .closed: return true
            case .line: continue
            case .timeout: return false
            }
        }
        return false
    }

    /// Every line until nothing more comes for `quiet` seconds.
    func drain(_ quiet: Double = 0.3) -> [[String: Any]] {
        var out: [[String: Any]] = []
        while case .line(let l) = read(quiet) { out.append(l) }
        return out
    }
}

/// Whether this machine answers on loopback addresses other than 127.0.0.1 (Linux does; macOS has only
/// 127.0.0.1 unless aliases are added). The tests that play several devices need them.
func loopbackAliases() -> Bool {
    #if canImport(Glibc)
    let fd = socket(AF_INET, Int32(SOCK_STREAM.rawValue), 0)
    #else
    let fd = socket(AF_INET, SOCK_STREAM, 0)
    #endif
    guard fd >= 0 else { return false }
    defer { Glibc_close(fd) }
    var local = sockaddr_in()
    #if canImport(Darwin)
    local.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
    #endif
    local.sin_family = sa_family_t(AF_INET)
    inet_pton(AF_INET, "127.0.1.1", &local.sin_addr)
    return withUnsafePointer(to: &local) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
    } == 0
}
