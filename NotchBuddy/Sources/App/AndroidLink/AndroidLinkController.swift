#if ANDROID_LINK
import AppKit
import CoreImage
import CoreImage.CIFilterBuiltins

// MARK: - Android link: switch, lifecycle, pairing
//
// Settings → Android phone. Off by default; while off nothing listens, nothing is announced and no
// timer runs. On: the identity and the token come from the Keychain (or the link does not start),
// the server listens, the island's state is fed to the hub. The pairing code (the token) is shown
// only after a click, and only in the settings window. "Pair again" disconnects every phone first,
// then makes a new token.

@MainActor
final class AndroidLinkController: ObservableObject {
    static let shared = AndroidLinkController()
    static let enabledKey = "androidLinkEnabled"

    struct Pairing {
        let link: String
        let qr: NSImage?
        let host: String
        let port: UInt16
        let name: String
    }

    @Published private(set) var enabled = UserDefaults.standard.bool(forKey: AndroidLinkController.enabledKey)
    @Published private(set) var port: UInt16?
    @Published private(set) var error: String?
    @Published private(set) var clients = 0
    /// Shown only after "Show pairing code"; forgotten when the link stops or the settings close it.
    @Published private(set) var pairing: Pairing?

    private var shared: LinkShared?
    private var server: LinkServer?
    private var feed: LinkFeed?
    private var fingerprint = ""
    private let secrets = KeychainSecretStore()
    private let identities = KeychainIdentityStore()

    var running: Bool { server != nil }

    func startIfEnabled() {
        if enabled { start() }
    }

    func setEnabled(_ on: Bool) {
        enabled = on
        UserDefaults.standard.set(on, forKey: Self.enabledKey)
        if on { start() } else { stop() }
    }

    private func start() {
        guard server == nil else { return }
        do {
            try KeychainIdentityStore.check()
            let identity = try LinkPairing.loadOrCreateIdentity(identities)
            guard let secIdentity = identities.secIdentity() else { throw LinkError.noIdentity }
            let token = try LinkPairing.token(secrets)
            let hub = LinkHub(host: MacLinkHost())
            let shared = LinkShared(hub: hub, token: token, name: Self.computerName())
            let server = LinkServer(shared: shared, identity: secIdentity, fingerprint: identity.fingerprint)
            server.onState = { [weak self] result in self?.serverState(result) }
            let feed = LinkFeed(hub: hub)
            self.shared = shared
            self.server = server
            self.feed = feed
            fingerprint = identity.fingerprint
            error = nil
            server.start()
            feed.start()
        } catch {
            self.error = LinkKeychain.describe(error)
            linkLog("could not start")
        }
    }

    private func stop() {
        feed?.stop()
        server?.stop()
        feed = nil
        server = nil
        shared = nil
        port = nil
        pairing = nil
        clients = 0
        error = nil
        linkLog("stopped")
    }

    /// The app is quitting: withdraw the announcement so phones forget this computer at once.
    func stopOnExit() {
        server?.stop()
    }

    private func serverState(_ result: Result<UInt16, LinkError>) {
        guard server != nil else { return }
        switch result {
        case .success(let p):
            port = p
            error = nil
        case .failure(let e):
            port = nil
            error = LinkKeychain.describe(e)
        }
    }

    /// For the settings window while it is open (it asks every few seconds; nothing runs otherwise).
    func refresh() {
        clients = shared?.hub.clients ?? 0
    }

    // MARK: Pairing

    func showPairing() {
        guard let shared, let port else { return }
        guard let host = LinkPairing.lanAddress() else {
            error = String(localized: "android.error.no-network")
            return
        }
        let link = LinkPairing.link(host: host, port: port, fingerprint: fingerprint, token: shared.currentToken, name: shared.name)
        pairing = Pairing(link: link, qr: Self.qrImage(link), host: host, port: port, name: shared.name)
    }

    func hidePairing() {
        pairing = nil
    }

    /// A new pairing code. The phones go first and unconditionally: if the Keychain cannot store the new
    /// token, "Pair again" must still have cut the old phone off, and the error is shown afterwards.
    func pairAgain() {
        guard let shared else { return }
        shared.hub.kickAll(code: "auth", message: "unpaired")
        do {
            shared.replaceToken(try LinkPairing.newToken(secrets))
            linkLog("new pairing code, phones disconnected")
            showPairing()
        } catch {
            self.error = LinkKeychain.describe(error)
            pairing = nil
        }
    }

    static func computerName() -> String {
        LinkPairing.displayName(Host.current().localizedName ?? ProcessInfo.processInfo.hostName)
    }

    /// The pairing link as a QR code (CoreImage, no library), black on white, sharp when scaled.
    static func qrImage(_ text: String) -> NSImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage else { return nil }
        let scaled = output.transformed(by: CGAffineTransform(scaleX: 8, y: 8))
        let rep = NSCIImageRep(ciImage: scaled)
        let image = NSImage(size: rep.size)
        image.addRepresentation(rep)
        return image
    }
}
#endif
