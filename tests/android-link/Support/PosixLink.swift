import Foundation
import Synchronization
#if canImport(Glibc)
import Glibc
#elseif canImport(Darwin)
import Darwin
#endif

// A plain TCP server for the core, with POSIX sockets: what LinkServer does with Network.framework on
// the Mac, minus TLS (the interop server puts socat's TLS in front; the tests talk to it directly).
// Test code only: the app never listens without TLS.

final class PosixTransport: LinkTransport, @unchecked Sendable {
    let fd: Int32
    private let writes = DispatchQueue(label: "posix.write")
    private let closed = Atomic<Bool>(false)

    init(fd: Int32) { self.fd = fd }

    func send(_ bytes: Data, done: @escaping @Sendable (Bool) -> Void) {
        writes.async { [fd] in
            var ok = true
            bytes.withUnsafeBytes { raw in
                var off = 0
                while off < raw.count {
                    #if canImport(Glibc)
                    let n = Glibc.send(fd, raw.baseAddress! + off, raw.count - off, Int32(MSG_NOSIGNAL))
                    #else
                    let n = Darwin.send(fd, raw.baseAddress! + off, raw.count - off, 0)
                    #endif
                    if n <= 0 { ok = false; return }
                    off += n
                }
            }
            done(ok)
        }
    }

    func close() {
        writes.async { [self] in
            guard !closed.exchange(true, ordering: .sequentiallyConsistent) else { return }
            shutdown(fd, Int32(SHUT_RDWR))
        }
    }
}

final class PosixServer: @unchecked Sendable {
    let shared: LinkShared
    let listenFd: Int32
    let port: UInt16
    private let stopped = Atomic<Bool>(false)
    /// What address the server pretends each connection comes from (tests of the admission rules); nil: the real one.
    let sourceOverride = Mutex<String?>(nil)

    init(shared: LinkShared, host: String = "127.0.0.1", port: UInt16 = 0) throws {
        self.shared = shared
        #if canImport(Glibc)
        let fd = socket(AF_INET, Int32(SOCK_STREAM.rawValue), 0)
        #else
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        #endif
        guard fd >= 0 else { throw LinkError.cannotListen("socket") }
        var one: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, socklen_t(MemoryLayout<Int32>.size))
        var addr = sockaddr_in()
        #if canImport(Darwin)
        addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        #endif
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = in_port_t(port).bigEndian
        inet_pton(AF_INET, host, &addr.sin_addr)
        let bound = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
        }
        guard bound == 0, listen(fd, 32) == 0 else {
            Glibc_close(fd)
            throw LinkError.cannotListen("bind")
        }
        var got = sockaddr_in()
        var len = socklen_t(MemoryLayout<sockaddr_in>.size)
        _ = withUnsafeMutablePointer(to: &got) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(fd, $0, &len) }
        }
        listenFd = fd
        self.port = UInt16(bigEndian: got.sin_port)
        let thread = Thread { [self] in acceptLoop() }
        thread.start()
    }

    func stop() {
        guard !stopped.exchange(true, ordering: .sequentiallyConsistent) else { return }
        shutdown(listenFd, Int32(SHUT_RDWR))
        Glibc_close(listenFd)
        shared.hub.kickAll(code: "closed", message: "the phone link was turned off")
    }

    private func acceptLoop() {
        while !stopped.load(ordering: .sequentiallyConsistent) {
            var peer = sockaddr_in()
            var len = socklen_t(MemoryLayout<sockaddr_in>.size)
            let fd = withUnsafeMutablePointer(to: &peer) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { accept(listenFd, $0, &len) }
            }
            guard fd >= 0 else {
                if stopped.load(ordering: .sequentiallyConsistent) { return }
                continue
            }
            let real = withUnsafeBytes(of: &peer.sin_addr) { LinkAddress.key(v4: Array($0)) }
            let source = sourceOverride.withLock { $0 } ?? real
            // Same order as LinkServer: the local-network filter, then admission, before any byte is read.
            guard LinkAddress.isLocal(source), let ticket = shared.admission.admit(LinkAddress.key(source)) else {
                Glibc_close(fd)
                continue
            }
            serve(fd: fd, ticket: ticket)
        }
    }

    private func serve(fd: Int32, ticket: Ticket) {
        let queue = DispatchQueue(label: "posix.conversation")
        let transport = PosixTransport(fd: fd)
        let conversation = Conversation(shared: shared, transport: transport, ticket: ticket, queue: queue)
        let done = Atomic<Bool>(false)
        conversation.onFinish = { done.store(true, ordering: .sequentiallyConsistent) }
        queue.async { conversation.start() }
        let reader = Thread {
            var buffer = [UInt8](repeating: 0, count: 16 * 1024)
            while true {
                let n = buffer.withUnsafeMutableBytes { recv(fd, $0.baseAddress, $0.count, 0) }
                if n <= 0 { break }
                let chunk = Data(buffer[0..<n])
                queue.async { conversation.received(chunk) }
            }
            queue.async { conversation.receivedEnd() }
            // The descriptor is closed once the conversation is over (its writer may still be flushing).
            while !done.load(ordering: .sequentiallyConsistent) { usleep(5_000) }
            Glibc_close(fd)
        }
        reader.start()
    }
}

/// `close(2)`, out of the way of the methods called `close`.
func Glibc_close(_ fd: Int32) {
    #if canImport(Glibc)
    _ = Glibc.close(fd)
    #else
    _ = Darwin.close(fd)
    #endif
}
