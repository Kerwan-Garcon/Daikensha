#if !APPSTORE
import AppKit

// MARK: - LiveMusicControl

final class LiveMusicControl: MusicControlling, @unchecked Sendable {
    var isMusicRunning: Bool {
        NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.Music").count > 0
    }
    var isSpotifyRunning: Bool {
        NSRunningApplication.runningApplications(withBundleIdentifier: "com.spotify.client").count > 0
    }
    var isYouTubeMusicConnected: Bool {
        MainActor.assumeIsolated { YouTubeMusicController.shared.link == .connected }
    }

    private enum Player { case appleMusic, spotify, youTubeMusic }

    /// YouTube Music when it plays; otherwise Apple Music when it runs (as before);
    /// otherwise YouTube Music when Pear is connected and Spotify is silent; otherwise Spotify.
    @MainActor private var player: Player {
        let youTube = YouTubeMusicController.shared
        if youTube.isPlaying { return .youTubeMusic }
        if isMusicRunning { return .appleMusic }
        if youTube.link == .connected && !SpotifyController.shared.isPlaying { return .youTubeMusic }
        return .spotify
    }

    @MainActor func play() {
        switch player {
        case .appleMusic:   MusicController.shared.play()
        case .spotify:      SpotifyController.shared.play()
        case .youTubeMusic: YouTubeMusicController.shared.play()
        }
    }
    @MainActor func pause() {
        switch player {
        case .appleMusic:   MusicController.shared.pause()
        case .spotify:      SpotifyController.shared.pause()
        case .youTubeMusic: YouTubeMusicController.shared.pause()
        }
    }
    @MainActor func nextTrack() {
        switch player {
        case .appleMusic:   MusicController.shared.nextTrack()
        case .spotify:      SpotifyController.shared.nextTrack()
        case .youTubeMusic: YouTubeMusicController.shared.nextTrack()
        }
    }
    @MainActor func previousTrack() {
        switch player {
        case .appleMusic:   MusicController.shared.previousTrack()
        case .spotify:      SpotifyController.shared.previousTrack()
        case .youTubeMusic: YouTubeMusicController.shared.previousTrack()
        }
    }
    @MainActor func volumeUp()   { adjustVolume(by: +25) }
    @MainActor func volumeDown() { adjustVolume(by: -25) }
    @MainActor private func adjustVolume(by delta: Int) {
        switch player {
        case .appleMusic:   MusicController.shared.adjustVolume(by: delta)
        case .spotify:      SpotifyController.shared.adjustVolume(by: delta)
        case .youTubeMusic: YouTubeMusicController.shared.adjustVolume(by: delta)
        }
    }
    @MainActor func setVolume(_ pct: Int) {
        switch player {
        case .appleMusic:   MusicController.shared.setVolume(pct)
        case .spotify:      SpotifyController.shared.setVolume(pct)
        case .youTubeMusic: YouTubeMusicController.shared.setVolume(pct)
        }
    }
    @MainActor func playSearch(_ name: String) async -> Bool {
        if player == .youTubeMusic { return await YouTubeMusicController.shared.searchAndPlay(name) }
        guard isMusicRunning else { return false }
        return await MusicController.shared.playSearch(name)
    }
    @MainActor func playPlaylist(_ name: String) async -> Bool {
        guard isMusicRunning else { return false }
        return await MusicController.shared.playPlaylist(name)
    }
    @MainActor func launchAndPlay() async {
        if !isMusicRunning {
            NSWorkspace.shared.open(URL(fileURLWithPath: "/System/Applications/Music.app"))
            try? await Task.sleep(nanoseconds: 1_500_000_000)
        }
        if isMusicRunning { MusicController.shared.play() }
    }
    @MainActor func launchSpotify() async {
        guard !isSpotifyRunning else { SpotifyController.shared.play(); return }
        let url = URL(fileURLWithPath: "/Applications/Spotify.app")
        NSWorkspace.shared.open(url)
        try? await Task.sleep(nanoseconds: 1_500_000_000)
        if isSpotifyRunning { SpotifyController.shared.play() }
    }
    @MainActor func launchYouTubeMusic() async {
        let youTube = YouTubeMusicController.shared
        youTube.openYouTubeMusic()
        // Pear connects a moment after its launch (YouTubeMusicController.retrySoon)
        for _ in 0..<10 where youTube.link != .connected {
            try? await Task.sleep(nanoseconds: 1_000_000_000)
        }
        if youTube.link == .connected { youTube.play() }
    }
    @MainActor func openSearch(_ name: String) {
        if player == .youTubeMusic {
            var comps = URLComponents(string: "https://music.youtube.com/search")!
            comps.queryItems = [URLQueryItem(name: "q", value: name)]
            if let url = comps.url { NSWorkspace.shared.open(url) }
            return
        }
        guard let encoded = name.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed),
              let url = URL(string: "https://music.apple.com/search?term=\(encoded)") else { return }
        // Open in Apple Music app, not the browser
        if let appURL = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.apple.Music") {
            let config = NSWorkspace.OpenConfiguration()
            NSWorkspace.shared.open([url], withApplicationAt: appURL, configuration: config)
        } else {
            NSWorkspace.shared.open(url)
        }
    }
}

// MARK: - LivePillControl

@MainActor
final class LivePillControl: PillControlling {
    func activeIds()  -> Set<String> { AppState.shared.activeIntegrations }
    func mainPillId() -> String      { AppState.shared.mainPillId }
    func activeCount() -> Int        { AppState.shared.activeIntegrations.count }
    func toggleIntegration(_ id: String) { AppState.shared.toggleIntegration(id) }
    func setMainPill(_ id: String)       { AppState.shared.setMainPill(id) }
}

// MARK: - Configuration

extension VoiceActionRunner {
    func configureLive() {
        music = LiveMusicControl()
        pills = LivePillControl()
        info  = LiveVoiceInfo.shared
        onLanguageSwitch = { lang in
            VoiceSettings.language = lang          // Settings picker follows (same key)
            VoiceBrain.shared.endConversation()    // next session gets the new instructions
        }
    }
}
#endif
