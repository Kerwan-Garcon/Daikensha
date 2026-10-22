import Foundation
import CoreGraphics

/// How Mochi's face is drawn. `.signature` gives him the traits that make him Mochi rather
/// than one more pill-eyed blob: a small mouth that follows his mood, a glint in his eyes,
/// rice-cake pink cheeks and a few idle quirks. `.portrait` keeps all of that and makes him
/// look like his owner: tan skin, braids, sunglasses pushed up on his head, a red hair clip,
/// a light beard, a black tee, and eyebrows that say how each state feels.
/// `.classic` is the face that shipped before.
/// Only the main Mochi wears it; the mini Mochis keep their flat look.
enum MochiLook: String, CaseIterable {
    case signature, portrait, classic

    var displayName: String {
        switch self {
        case .signature: return "Signature"
        case .portrait:  return "Portrait"
        case .classic:   return "Classic"
        }
    }

    /// The next face in the wardrobe switch.
    var next: MochiLook {
        let all = MochiLook.allCases
        return all[(all.firstIndex(of: self)! + 1) % all.count]
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
    case grit       // teeth out — cross (portrait look)

    static func forEyes(_ eye: EyeShape, speaking: Bool, portrait: Bool = false) -> MochiMouth {
        if speaking { return .talking }
        if portrait && (eye == .line || eye == .flat) { return .grit }
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

/// Where the portrait look's eyebrows sit, per state and per emote. Units are fractions of
/// the body radius R; angles are radians.
struct MochiBrows: Equatable {
    var lift: CGFloat    // up from rest
    var angle: CGFloat   // + inner ends down (cross), − inner ends up (worried)
    var arch: CGFloat    // how much each brow bows upward
    var asym: CGFloat    // extra lift of the right brow alone (a raised eyebrow)

    static func pose(state: BotState, eye: EyeShape, t: Double) -> MochiBrows {
        // An emote's eyes win: they say what he feels right now.
        switch eye {
        case .line:  return MochiBrows(lift: -0.04, angle: 0.45, arch: 0, asym: 0)       // annoyed
        case .dot:   return MochiBrows(lift: 0.11, angle: -0.1, arch: 0.35, asym: 0)     // surprised
        case .heart: return MochiBrows(lift: 0.06, angle: -0.2, arch: 0.3, asym: 0)      // in love
        case .star:  return MochiBrows(lift: 0.05, angle: 0.1, arch: 0.25, asym: 0.04)   // proud
        case .wink:  return MochiBrows(lift: 0.02, angle: 0.1, arch: 0.2, asym: 0.07)    // smug
        case .happy: return MochiBrows(lift: 0.07, angle: -0.12, arch: 0.4, asym: 0)     // delighted
        default: break
        }
        switch state {
        case .idle:      return MochiBrows(lift: 0, angle: 0.05, arch: 0.22, asym: 0)
        case .working:   return MochiBrows(lift: -0.02, angle: 0.2, arch: 0.12, asym: 0)   // focused
        case .thinking:  return MochiBrows(lift: 0.02, angle: 0.12, arch: 0.2, asym: 0.08) // hmm
        case .searching: return MochiBrows(lift: 0.04, angle: -0.04, arch: 0.2, asym: 0)
        case .approval:  return MochiBrows(lift: 0.09, angle: -0.16, arch: 0.3, asym: 0)   // waiting for you
        case .question:  return MochiBrows(lift: 0.05, angle: -0.08, arch: 0.25, asym: 0.07)
        case .error:     return MochiBrows(lift: -0.04, angle: 0.45, arch: 0, asym: 0)     // cross
        case .finished:  return MochiBrows(lift: 0.07, angle: -0.12, arch: 0.4, asym: 0)
        case .ratelimit: return MochiBrows(lift: 0.03, angle: -0.4, arch: 0.1, asym: 0)    // worried
        case .sleeping:  return MochiBrows(lift: -0.03, angle: -0.06, arch: 0.15, asym: 0)
        case .dizzy:     return MochiBrows(lift: 0.02, angle: 0.3 * CGFloat(sin(t * 6)), arch: 0.15,
                                           asym: 0.06 * CGFloat(sin(t * 4)))
        }
    }
}
