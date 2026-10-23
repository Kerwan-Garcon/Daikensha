#if ANDROID_LINK
import Combine
import SwiftUI

// MARK: - Settings → Android phone
//
// A new section in the style of the "iPhone" one beside it: a switch, a line of explanation, and,
// while the link is on, its state and the pairing code behind a click.

struct AndroidLinkSettingsView: View {
    @ObservedObject private var link = AndroidLinkController.shared

    var body: some View {
        GroupBox(String(localized: "android.title")) {
            VStack(alignment: .leading, spacing: 6) {
                Toggle(String(localized: "android.link.toggle"), isOn: Binding(
                    get: { link.enabled },
                    set: { link.setEnabled($0) }))
                Text(String(localized: "android.link.description"))
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if link.enabled {
                    status
                }
            }
            .padding(6)
        }
    }

    @ViewBuilder private var status: some View {
        if let error = link.error {
            Text(error)
                .font(.system(size: 11))
                .foregroundColor(.orange)
                .fixedSize(horizontal: false, vertical: true)
        } else if let port = link.port {
            Text(String(format: String(localized: "android.status.listening %lld %lld"), Int64(port), Int64(link.clients)))
                .font(.system(size: 11))
                .foregroundColor(.secondary)
                .onAppear { link.refresh() }
                // The count of phones is read every few seconds, only while this view is on screen.
                .onReceive(Timer.publish(every: 3, on: .main, in: .common).autoconnect()) { _ in link.refresh() }
            pairingSection
        } else {
            Text(String(localized: "android.status.starting"))
                .font(.system(size: 11))
                .foregroundColor(.secondary)
        }
    }

    @ViewBuilder private var pairingSection: some View {
        if let pairing = link.pairing {
            VStack(alignment: .leading, spacing: 6) {
                if let qr = pairing.qr {
                    Image(nsImage: qr)
                        .interpolation(.none)
                        .resizable()
                        .frame(width: 180, height: 180)
                        .accessibilityLabel(String(localized: "android.pair.qr"))
                }
                Text(String(localized: "android.pair.hint"))
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                HStack(spacing: 8) {
                    Button(String(localized: "android.pair.copy")) {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(pairing.link, forType: .string)
                    }
                    .buttonStyle(.bordered)
                    Button(String(localized: "android.pair.again")) { link.pairAgain() }
                        .buttonStyle(.bordered)
                    Button(String(localized: "android.pair.hide")) { link.hidePairing() }
                        .buttonStyle(.bordered)
                }
            }
        } else {
            Button(String(localized: "android.pair.show")) { link.showPairing() }
                .buttonStyle(.bordered)
        }
    }
}
#endif
