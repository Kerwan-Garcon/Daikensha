#if !APPSTORE
import Foundation
import AppKit
import Combine

// MARK: - YouTube Music Controller

/// The song YouTube Music has loaded, as Pear Desktop's API Server describes it.
struct YouTubeMusicTrack: Equatable, Sendable {
    let id: String          // the YouTube videoId
    let title: String
    let artist: String
    let album: String
    let duration: Double    // seconds
    var artworkURL: String?
}

/// YouTube Music has no official desktop app and no AppleScript dictionary. Coucou drives
/// Pear Desktop (github.com/pear-devs/pear-desktop, formerly th-ch/youtube-music) through its
/// "API Server" plugin: REST commands and a WebSocket on 127.0.0.1:26538, never anywhere else.
/// The first connection is a click in Coucou, then "Allow" in Pear's own dialog; the token it
/// returns lives in the Keychain.
/// Singleton, @MainActor, GitHub build only. Nothing runs on a timer: state arrives on the
/// WebSocket while Pear runs, the card extrapolates the position.
@MainActor
final class YouTubeMusicController: ObservableObject {
    static let shared = YouTubeMusicController()
    nonisolated static let pillId    = "integration_youtube_music"
    nonisolated static let bundleIds: Set<String> = ["com.github.th-ch.youtube-music"]
    nonisolated static let red       = "#FF0033"
    nonisolated static let port      = 26538
    nonisolated static let clientId  = "coucou"
    nonisolated static let tokenKey  = "youtube-music-token"
    nonisolated static let downloadURL = URL(string: "https://github.com/pear-devs/pear-desktop/releases/latest")!

    /// Where Coucou stands with Pear's API Server.
    enum Link: Equatable {
        case idle          // pill off, or Pear not running
        case unreachable   // Pear runs but its API Server plugin doesn't answer
        case needsAuth     // no token yet: waits for the user's click
        case waiting       // Pear shows its "Allow" dialog
        case denied        // the user clicked "Deny" in Pear
        case connected
    }

    @Published private(set) var link: Link = .idle
    @Published private(set) var track: YouTubeMusicTrack?
    @Published private(set) var isPlaying = false
    /// Position (seconds) at `positionDate`; while playing, the real position runs on from there.
    @Published private(set) var positionAnchor: Double = 0
    @Published private(set) var positionDate = Date()
    @Published private(set) var shuffling = false
    /// NONE, ALL or ONE, as YouTube Music cycles them.
    @Published private(set) var repeatMode = "NONE"
    @Published private(set) var volume: Int = 50
    @Published private(set) var artwork: NSImage?

    var repeating: Bool { repeatMode != "NONE" }

    private var notifTokens: [Any] = []
    private var cancellables = Set<AnyCancellable>()
    private var socket: URLSessionWebSocketTask?
    /// Bumped on every connect and disconnect, so a stale socket's last words are ignored.
    private var generation = 0
    private var launchRetry: Task<Void, Never>?
    private var artworkCache: [String: NSImage] = [:]
    private var artworkCacheOrder: [String] = []
    private var artworkLoadingId: String?
    private var pendingVolume: Int?
    private var volumeTask: Task<Void, Never>?

    /// Commands: Pear answers at once or not at all.
    private let session = YouTubeMusicController.localSession(timeout: 5)
    /// The "Allow" dialog waits for the user; the WebSocket stays open.
    private let slowSession = YouTubeMusicController.localSession(timeout: 120)

    nonisolated private static func localSession(timeout: TimeInterval) -> URLSession {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = timeout
        config.connectionProxyDictionary = [:]   // 127.0.0.1 only, never through a proxy
        return URLSession(configuration: config)
    }

    private var isPillActive: Bool {
        AppState.shared.activeIntegrations.contains(Self.pillId)
    }

    private var token: String? { KeychainStore.shared.get(Self.tokenKey) }

    /// Pear already gave Coucou a token: "Try again" reconnects rather than asking again.
    var hasToken: Bool { token != nil }

    var isInstalled: Bool {
        Self.bundleIds.contains { NSWorkspace.shared.urlForApplication(withBundleIdentifier: $0) != nil }
    }

    var isRunning: Bool {
        NSWorkspace.shared.runningApplications.contains { Self.bundleIds.contains($0.bundleIdentifier ?? "") }
    }

    /// Where playback is now, extrapolated from the last anchor.
    func position(at date: Date) -> Double {
        let elapsed = isPlaying ? date.timeIntervalSince(positionDate) : 0
        let p = positionAnchor + max(0, elapsed)
        guard let d = track?.duration, d > 0 else { return p }
        return min(p, d)
    }

    private init() {
        // Pear launched: its API Server starts a moment after the window
        let tok1 = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didLaunchApplicationNotification,
            object: nil,
            queue: .main
        ) { notif in
            let bundleId = (notif.userInfo?[NSWorkspace.applicationUserInfoKey]
                as? NSRunningApplication)?.bundleIdentifier ?? ""
            guard YouTubeMusicController.bundleIds.contains(bundleId) else { return }
            Task { @MainActor in YouTubeMusicController.shared.retrySoon() }
        }
        notifTokens.append(tok1)

        // Pear quit: nothing is playing anymore
        let tok2 = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didTerminateApplicationNotification,
            object: nil,
            queue: .main
        ) { notif in
            let bundleId = (notif.userInfo?[NSWorkspace.applicationUserInfoKey]
                as? NSRunningApplication)?.bundleIdentifier ?? ""
            guard YouTubeMusicController.bundleIds.contains(bundleId) else { return }
            Task { @MainActor in YouTubeMusicController.shared.disconnect(to: .idle) }
        }
        notifTokens.append(tok2)

        // Pill turned on → connect if Pear runs. Pill turned off → hang up and clear.
        AppState.shared.$activeIntegrations
            .map { $0.contains(Self.pillId) }
            .removeDuplicates()
            .sink { [weak self] active in
                guard let self else { return }
                // The publisher fires before the property is set: act once it is
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    if active { self.refresh() } else { self.disconnect(to: .idle) }
                }
            }
            .store(in: &cancellables)
    }

    // MARK: - Connection

    /// Connects when it can and isn't already. Called when the pill turns on, when the card
    /// appears and from "Try again". Never launches Pear.
    func refresh() {
        guard isPillActive else { return }
        if link == .connected || link == .waiting { return }
        guard let token else {
            // A first connection needs the user's click (Pear shows a dialog)
            if isRunning { setLink(.needsAuth); return }
            // Not seen by its bundle id (a fork, another build): ask the port itself
            Task {
                let answers = await serverAnswers()
                guard isPillActive, self.token == nil, link != .waiting else { return }
                setLink(answers ? .needsAuth : .idle)
            }
            return
        }
        openSocket(token: token)
    }

    /// Anything answers HTTP on Pear's port (a 401 without a token counts).
    private func serverAnswers() async -> Bool {
        var request = URLRequest(url: Self.url("/api/v1/song-info"))
        request.timeoutInterval = 2
        return ((try? await session.data(for: request))?.1 as? HTTPURLResponse) != nil
    }

    /// Pear takes a moment to (re)start its server: try a few times, then wait for the card or a click.
    private func retrySoon() {
        launchRetry?.cancel()
        launchRetry = Task { @MainActor [weak self] in
            for delay in [2, 4, 8] {
                try? await Task.sleep(for: .seconds(delay))
                guard let self, !Task.isCancelled, self.isPillActive else { return }
                if self.link == .connected { return }
                self.refresh()
            }
        }
    }

    /// "Connect": asks Pear for a token. Pear shows "Allow / Deny" to the user; nothing is
    /// approved on its behalf.
    func authorize() {
        guard isPillActive, link != .waiting else { return }
        setLink(.waiting)
        var request = URLRequest(url: Self.url("/auth/\(Self.clientId)"))
        request.httpMethod = "POST"
        request.timeoutInterval = 120
        Task {
            let result = try? await slowSession.data(for: request)
            guard isPillActive else { return }
            let status = (result?.1 as? HTTPURLResponse)?.statusCode
            if status == 200,
               let data = result?.0,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let token = json["accessToken"] as? String, !token.isEmpty {
                KeychainStore.shared.set(Self.tokenKey, value: token)
                // Still "waiting" until Pear's first message says connected
                openSocket(token: token)
            } else if status == 403 {
                setLink(.denied)
            } else {
                setLink(isRunning ? .unreachable : .idle)
            }
        }
    }

    private func openSocket(token: String) {
        closeSocket()
        generation += 1
        let gen = generation
        var comps = URLComponents(url: Self.url("/api/v1/ws"), resolvingAgainstBaseURL: false)!
        comps.queryItems = [URLQueryItem(name: "token", value: token)]
        let task = slowSession.webSocketTask(with: comps.url!)
        socket = task
        task.resume()
        Task { @MainActor [weak self] in
            while true {
                let message: URLSessionWebSocketTask.Message
                do {
                    message = try await task.receive()
                } catch {
                    self?.socketEnded(task, generation: gen)
                    return
                }
                guard let self, self.generation == gen else { return }
                switch message {
                case .string(let text): self.handle(text)
                case .data(let data): self.handle(String(decoding: data, as: UTF8.self))
                @unknown default: break
                }
            }
        }
    }

    private func socketEnded(_ task: URLSessionWebSocketTask, generation gen: Int) {
        guard generation == gen else { return }
        socket = nil
        let wasConnected = link == .connected
        if task.closeCode == .policyViolation {
            // 1008: Pear no longer knows this token (its secret changed, or the client was removed)
            KeychainStore.shared.remove(Self.tokenKey)
            disconnect(to: .needsAuth)
        } else {
            disconnect(to: isRunning ? .unreachable : .idle)
            // Dropped while Pear still runs (its plugin restarted, a settings change): once more
            if wasConnected && isRunning { retrySoon() }
        }
    }

    private func closeSocket() {
        socket?.cancel(with: .normalClosure, reason: nil)
        socket = nil
    }

    private func disconnect(to newLink: Link) {
        generation += 1
        closeSocket()
        clearState()
        setLink(newLink)
    }

    private func setLink(_ newLink: Link) {
        if link != newLink { link = newLink }
    }

    // MARK: - State

    /// One WebSocket message: PLAYER_INFO first, then a message per change.
    private func handle(_ text: String) {
        guard isPillActive,
              let json = try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any],
              let type = json["type"] as? String else { return }
        let position = (json["position"] as? NSNumber)?.doubleValue
        switch type {
        case "PLAYER_INFO":
            setLink(.connected)
            if let song = json["song"] as? [String: Any], let t = Self.parseSong(song, previous: track) {
                setTrack(t)
            } else {
                clearState()
            }
            if let position { anchor(position) }
            setPlaying(((json["isPlaying"] as? Bool) ?? false) && track != nil)
            if let v = json["shuffle"] as? Bool { shuffling = v }
            if let r = json["repeat"] as? String { repeatMode = r }
            applyVolume(json)
        case "VIDEO_CHANGED":
            if let song = json["song"] as? [String: Any], let t = Self.parseSong(song, previous: track) {
                setTrack(t)
                anchor(position ?? 0)
                if let paused = song["isPaused"] as? Bool { setPlaying(!paused) }
            }
        case "PLAYER_STATE_CHANGED":
            if let position { anchor(position) }
            setPlaying(((json["isPlaying"] as? Bool) ?? false) && track != nil)
        case "POSITION_CHANGED":
            // Pear says this every second while playing: only a seek is worth a redraw
            if let position, abs(position - self.position(at: Date())) > 1.5 { anchor(position) }
        case "VOLUME_CHANGED":
            applyVolume(json)
        case "REPEAT_CHANGED":
            if let r = json["repeat"] as? String { repeatMode = r }
        case "SHUFFLE_CHANGED":
            if let v = json["shuffle"] as? Bool { shuffling = v }
        default:
            break
        }
    }

    private func applyVolume(_ json: [String: Any]) {
        guard pendingVolume == nil, let v = (json["volume"] as? NSNumber)?.doubleValue else { return }
        let muted = (json["muted"] as? Bool) ?? false
        let value = muted ? 0 : Int(v.rounded())
        if volume != value { volume = min(100, max(0, value)) }
    }

    /// Pear's SongInfo → the track, or nil when nothing is loaded.
    static func parseSong(_ song: [String: Any], previous: YouTubeMusicTrack?) -> YouTubeMusicTrack? {
        let id = song["videoId"] as? String ?? ""
        let title = song["title"] as? String ?? ""
        guard !id.isEmpty || !title.isEmpty else { return nil }
        let image = song["imageSrc"] as? String
        return YouTubeMusicTrack(
            id: id,
            title: SpotifyController.shortTitle(title),
            artist: SpotifyController.shortArtist(song["artist"] as? String ?? ""),
            album: song["album"] as? String ?? "",
            duration: (song["songDuration"] as? NSNumber)?.doubleValue ?? 0,
            artworkURL: image ?? (previous?.id == id ? previous?.artworkURL : nil)
        )
    }

    private func setTrack(_ newTrack: YouTubeMusicTrack) {
        let changed = track?.id != newTrack.id
        if track != newTrack { track = newTrack }
        if changed { artwork = artworkCache[newTrack.id] }
        if artwork == nil { loadArtwork(for: newTrack) }
        syncTaskName()
    }

    private func setPlaying(_ playing: Bool) {
        let wasPlaying = isPlaying
        if playing != wasPlaying {
            // Freeze or restart the clock at the current position
            anchor(position(at: Date()))
            isPlaying = playing
        }
        // Reveal only on transition from not-playing → playing
        if playing && !wasPlaying {
            NotificationCenter.default.post(name: .musicReveal, object: nil)
        }
    }

    private func anchor(_ position: Double) {
        positionAnchor = max(0, position)
        positionDate = Date()
    }

    private func clearState() {
        track = nil
        artwork = nil
        isPlaying = false
        anchor(0)
        syncTaskName()
    }

    private func syncTaskName() {
        guard let idx = AppState.shared.tasks.firstIndex(where: { $0.id == Self.pillId }) else { return }
        let title = track?.title ?? ""
        let name = title.isEmpty
            ? (PillCatalog.definition(for: Self.pillId)?.name ?? "YouTube Music")
            : title
        if AppState.shared.tasks[idx].name != name { AppState.shared.tasks[idx].name = name }
    }

    // MARK: - Artwork

    private func loadArtwork(for track: YouTubeMusicTrack) {
        if let cached = artworkCache[track.id] { artwork = cached; return }
        guard artworkLoadingId != track.id, let url = track.artworkURL.flatMap(Self.imageURL) else { return }
        artworkLoadingId = track.id
        let id = track.id
        Task {
            defer { if artworkLoadingId == id { artworkLoadingId = nil } }
            guard let (data, response) = try? await URLSession.shared.data(from: url),
                  (response as? HTTPURLResponse)?.statusCode == 200,
                  let image = NSImage(data: data) else { return }
            cacheArtwork(image, for: id)
            if self.track?.id == id { artwork = image }
        }
    }

    private func cacheArtwork(_ image: NSImage, for id: String) {
        artworkCache[id] = image
        artworkCacheOrder.removeAll { $0 == id }
        artworkCacheOrder.append(id)
        while artworkCacheOrder.count > 12 {
            artworkCache[artworkCacheOrder.removeFirst()] = nil
        }
    }

    /// Only Google's image CDNs: the URL comes from another app.
    nonisolated static func imageURL(_ string: String) -> URL? {
        guard let url = safeWebURL(string), url.scheme == "https",
              let host = url.host?.lowercased() else { return nil }
        let allowed = ["googleusercontent.com", "ytimg.com", "ggpht.com"]
        guard allowed.contains(where: { host == $0 || host.hasSuffix("." + $0) }) else { return nil }
        return url
    }

    // MARK: - Playback controls

    func playPause() {
        guard link == .connected else { return }
        setPlaying(!isPlaying)   // optimistic; the WebSocket confirms
        command("toggle-play")
    }

    func play() {
        guard link == .connected else { return }
        command("play")
    }

    func pause() {
        guard link == .connected else { return }
        command("pause")
    }

    func nextTrack() {
        guard link == .connected else { return }
        command("next")
    }

    func previousTrack() {
        guard link == .connected else { return }
        command("previous")
    }

    func adjustVolume(by delta: Int) {
        setVolume(volume + delta)
    }

    func seek(to seconds: Double) {
        guard link == .connected, let track, track.duration > 0 else { return }
        let target = min(max(0, seconds), max(0, track.duration - 1))
        anchor(target)
        command("seek-to", body: ["seconds": Int(target.rounded())])
    }

    /// YouTube Music's shuffle is a button: one press flips it.
    func setShuffling(_ on: Bool) {
        guard link == .connected, on != shuffling else { return }
        shuffling = on
        command("shuffle")
    }

    /// YouTube Music cycles NONE → ALL → ONE → NONE, one press per step.
    func setRepeating(_ on: Bool) {
        guard link == .connected, on != repeating else { return }
        let presses = Self.repeatPresses(from: repeatMode, on: on)
        repeatMode = on ? "ALL" : "NONE"
        command("switch-repeat", body: ["iteration": presses])
    }

    nonisolated static func repeatPresses(from mode: String, on: Bool) -> Int {
        switch (mode, on) {
        case ("NONE", true): return 1   // → ALL
        case ("ALL", false): return 2   // → ONE → NONE
        case ("ONE", false): return 1   // → NONE
        default: return 0
        }
    }

    /// Coalesced: a slider drag sends at most one request every 120 ms.
    func setVolume(_ value: Int) {
        guard link == .connected else { return }
        let v = min(100, max(0, value))
        volume = v
        pendingVolume = v
        guard volumeTask == nil else { return }
        volumeTask = Task {
            while let v = pendingVolume {
                await send("volume", body: ["volume": v])
                try? await Task.sleep(for: .milliseconds(120))
                if pendingVolume == v { pendingVolume = nil }
            }
            volumeTask = nil
        }
    }

    private func command(_ path: String, body: [String: Int]? = nil) {
        Task { await send(path, body: body) }
    }

    private func send(_ path: String, body: [String: Int]?) async {
        guard let token else { return }
        var request = URLRequest(url: Self.url("/api/v1/\(path)"))
        request.httpMethod = "POST"
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        if let body {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try? JSONSerialization.data(withJSONObject: body)
        }
        let status = ((try? await session.data(for: request))?.1 as? HTTPURLResponse)?.statusCode
        if status == 401 {
            KeychainStore.shared.remove(Self.tokenKey)
            disconnect(to: .needsAuth)
        }
    }

    nonisolated private static func url(_ path: String) -> URL {
        URL(string: "http://127.0.0.1:\(port)\(path)")!
    }

    func openYouTubeMusic() {
        if let app = NSWorkspace.shared.runningApplications.first(where: { Self.bundleIds.contains($0.bundleIdentifier ?? "") }) {
            app.activate()
        } else if let url = Self.bundleIds.lazy.compactMap({ NSWorkspace.shared.urlForApplication(withBundleIdentifier: $0) }).first {
            NSWorkspace.shared.openApplication(at: url, configuration: .init(), completionHandler: nil)
        } else {
            openDownloadPage()
        }
    }

    func openDownloadPage() {
        NSWorkspace.shared.open(Self.downloadURL)
    }
}
#endif
