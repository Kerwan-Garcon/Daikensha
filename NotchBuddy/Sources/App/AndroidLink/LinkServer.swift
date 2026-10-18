#if ANDROID_LINK
import Foundation
import Network
import Security
import Synchronization

// MARK: - Android link: the TLS server on the local network
//
// Network.framework's NWListener with TLS 1.2/1.3 and the identity from the Keychain; each accepted
// connection is one `Conversation` (Core/), on a serial queue of its own. The port of `serve` and
// `one_connection` (server.rs):
//   * it listens on IPv4, port 47821, else any free port (the pairing link carries the port);
//   * a peer that is not on the local network (private, loopback, link-local) is cancelled before the
//     TLS handshake, and so is one the admission rules refuse (Core/Admission.swift);
//   * the TLS handshake gets the ticket's wait (10 s, 3 s when the pool fills up), then the hello the same;
//   * while it runs, the computer is announced with Bonjour (`_coucou._tcp`, TXT `v=1` and the short
//     certificate id only); stopping withdraws it.
// Off means nothing: no listener, no announcement, no timer.

final class LinkServer: @unchecked Sendable {
    private let shared: LinkShared
    private let identity: SecIdentity
    private let fingerprint: String
    private let queue = DispatchQueue(label: "fr.louisraille.NotchBuddy.android-link.listener")

    // Owned by `queue`.
    private var listener: NWListener?
    private var connections: [ObjectIdentifier: NWConnection] = [:]
    private var stopped = false

    private let portCell = Mutex<UInt16?>(nil)
    /// Called on the main actor when the server is listening (with its port) or failed for good.
    var onState: (@MainActor @Sendable (Result<UInt16, LinkError>) -> Void)?

    init(shared: LinkShared, identity: SecIdentity, fingerprint: String) {
        self.shared = shared
        self.identity = identity
        self.fingerprint = fingerprint
    }

    var port: UInt16? { portCell.withLock { $0 } }

    func start() {
        queue.async { [self] in listen(on: LinkProtocol.defaultPort, fallback: true) }
    }

    /// Stops listening, withdraws the announcement and closes every connection (the phones are told why).
    func stop() {
        shared.hub.kickAll(code: "closed", message: "the phone link was turned off")
        queue.async { [self] in
            stopped = true
            listener?.cancel()
            listener = nil
            // Connections that never authenticated are not in the hub: cut them here.
            for c in connections.values { c.cancel() }
            connections = [:]
            portCell.withLock { $0 = nil }
        }
    }

    private func parameters() -> NWParameters {
        let tls = NWProtocolTLS.Options()
        let sec = tls.securityProtocolOptions
        if let local = sec_identity_create(identity) {
            sec_protocol_options_set_local_identity(sec, local)
        }
        sec_protocol_options_set_min_tls_protocol_version(sec, .TLSv12)
        sec_protocol_options_set_max_tls_protocol_version(sec, .TLSv13)
        let tcp = NWProtocolTCP.Options()
        tcp.noDelay = true
        let params = NWParameters(tls: tls, tcp: tcp)
        // IPv4 only, like the reference: the pairing link and the announcement carry an IPv4 address.
        if let ip = params.defaultProtocolStack.internetProtocol as? NWProtocolIP.Options {
            ip.version = .v4
        }
        params.includePeerToPeer = false
        return params
    }

    private func listen(on port: UInt16, fallback: Bool) {
        guard !stopped else { return }
        let listener: NWListener
        do {
            listener = try NWListener(using: parameters(), on: port == 0 ? .any : NWEndpoint.Port(rawValue: port) ?? .any)
        } catch {
            report(.failure(.cannotListen("\(error)")))
            return
        }
        if let txt = LinkDiscovery.txt(fingerprint) {
            listener.service = NWListener.Service(name: LinkDiscovery.instanceName(shared.name), type: LinkDiscovery.serviceType,
                                                  domain: nil, txtRecord: NWTXTRecord(txt))
        }
        listener.stateUpdateHandler = { [weak self, weak listener] state in
            guard let self, let listener else { return }
            switch state {
            case .ready:
                let actual = listener.port?.rawValue ?? port
                self.portCell.withLock { $0 = actual }
                linkLog("listening on port \(actual)")
                self.report(.success(actual))
            case .failed(let error):
                listener.cancel()
                if self.listener === listener { self.listener = nil }
                if fallback, case .posix(let code) = error, code == .EADDRINUSE {
                    // 47821 is taken: any free port (the pairing link says which).
                    self.listen(on: 0, fallback: false)
                } else {
                    linkLog("cannot listen")
                    self.report(.failure(.cannotListen("\(error)")))
                }
            default:
                break
            }
        }
        listener.serviceRegistrationUpdateHandler = { change in
            if case .add = change { linkLog("announced on the local network") }
        }
        listener.newConnectionHandler = { [weak self] connection in
            self?.accept(connection)
        }
        self.listener = listener
        listener.start(queue: queue)
    }

    private func report(_ result: Result<UInt16, LinkError>) {
        guard let onState else { return }
        Task { @MainActor in onState(result) }
    }

    /// The local-network filter, then admission, before any byte is read. On `queue`.
    private func accept(_ connection: NWConnection) {
        guard !stopped, let address = Self.sourceKey(connection.endpoint),
              let ticket = shared.admission.admit(address) else {
            connection.cancel()
            return
        }
        let connQueue = DispatchQueue(label: "fr.louisraille.NotchBuddy.android-link.connection")
        let transport = NWLinkTransport(connection: connection)
        let conversation = Conversation(shared: shared, transport: transport, ticket: ticket, queue: connQueue)
        let id = ObjectIdentifier(connection)
        connections[id] = connection
        conversation.onFinish = { [weak self] in
            guard let self else { return }
            self.queue.async { self.connections[id] = nil }
        }
        // The TLS handshake gets the ticket's wait; then the conversation gives the hello the same.
        let handshake = HandshakeFlag()
        connQueue.asyncAfter(deadline: .now() + ticket.wait) {
            if !handshake.done { connection.cancel() }
        }
        connection.stateUpdateHandler = { state in
            switch state {
            case .ready:
                handshake.finish()
                conversation.start()
                Self.receive(connection, into: conversation)
            case .failed, .cancelled:
                handshake.finish()
                conversation.receivedEnd()
            default:
                break
            }
        }
        connection.start(queue: connQueue)
    }

    private static func receive(_ connection: NWConnection, into conversation: Conversation) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { data, _, isComplete, error in
            if let data, !data.isEmpty { conversation.received(data) }
            if isComplete || error != nil {
                conversation.receivedEnd()
            } else {
                receive(connection, into: conversation)
            }
        }
    }

    /// The peer's address as an admission key, or nil if it is not on the local network.
    static func sourceKey(_ endpoint: NWEndpoint) -> String? {
        guard case .hostPort(let host, _) = endpoint else { return nil }
        switch host {
        case .ipv4(let a):
            let bytes = Array(a.rawValue)
            return LinkAddress.isLocal(v4: bytes) ? LinkAddress.key(v4: bytes) : nil
        case .ipv6(let a):
            let bytes = Array(a.rawValue)
            return LinkAddress.isLocal(v6: bytes) ? LinkAddress.key(v6: bytes) : nil
        default:
            return nil
        }
    }
}

/// Whether a connection's TLS handshake ended (well or not) before its deadline.
private final class HandshakeFlag: Sendable {
    private let value = Atomic<Bool>(false)
    var done: Bool { value.load(ordering: .sequentiallyConsistent) }
    func finish() { value.store(true, ordering: .sequentiallyConsistent) }
}

/// A conversation's byte stream over an NWConnection (TLS already done).
final class NWLinkTransport: LinkTransport, @unchecked Sendable {
    private let connection: NWConnection

    init(connection: NWConnection) {
        self.connection = connection
    }

    func send(_ bytes: Data, done: @escaping @Sendable (Bool) -> Void) {
        connection.send(content: bytes, completion: .contentProcessed { error in done(error == nil) })
    }

    func close() {
        // What was queued goes first, then TLS close_notify and FIN, then the connection is released.
        let connection = self.connection
        connection.send(content: nil, contentContext: .finalMessage, isComplete: true, completion: .contentProcessed { _ in
            connection.cancel()
        })
    }
}
#endif
