import Foundation

/// How Mochi's face is drawn. `.signature` gives him the traits that make him Mochi rather
/// than one more pill-eyed blob: a small mouth that follows his mood, a glint in his eyes,
/// rice-cake pink cheeks and a few idle quirks. `.classic` is the face that shipped before.
/// Only the main Mochi wears it; the mini Mochis keep their flat look.
enum MochiLook: String, CaseIterable {
    case signature, classic

    var displayName: String {
        switch self {
        case .signature: return "Signature"
        case .classic:   return "Classic"
        }
    }

    // UserDefaults key "mochiLook", default "signature", unknown value → .signature
    static var stored: MochiLook {
        get {
            let raw = UserDefaults.standard.string(forKey: "mochiLook") ?? "signature"
            return MochiLook(rawValue: raw) ?? .signature
        }
        set {
            UserDefaults.standard.set(newValue.rawValue, forKey: "mochiLook")
        }
    }
}

/// The mouth Mochi makes, read from the eyes he is showing so the face always agrees.
enum MochiMouth: Equatable {
    case none       // mailbox, cup eyes: the box has its own slot
    case cat        // ω — at rest, at work
    case smile      // open D with a tongue — happy, proud, in love
    case oh         // small o — attentive, asking, sleeping
    case gasp       // big o — surprised
    case smirk      // one-sided — winking
    case flat       // straight — annoyed, error
    case wavy       // wobbly — tired, dizzy
    case content    // closed little smile — eyes shut
    case talking    // opens and closes — speaking aloud

    static func forEyes(_ eye: EyeShape, speaking: Bool) -> MochiMouth {
        if speaking { return .talking }
        switch eye {
        case .pill:                  return .cat
        case .wide:                  return .oh
        case .dot:                   return .gasp
        case .happy, .star, .heart:  return .smile
        case .wink:                  return .smirk
        case .line, .flat:           return .flat
        case .tired, .spiral:        return .wavy
        case .closed:                return .content
        case .cup:                   return .none
        }
    }
}
