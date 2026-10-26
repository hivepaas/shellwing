import SwiftUI
import UIKit

/// The desktop's app colors (crates/app/src/colors.rs, Dusk), by what each
/// one is for, so the phone looks like the same app.
enum Dusk {
    static let chrome = Color(hex: 0x21272f)
    static let panel = Color(hex: 0x252b34)
    static let page = Color(hex: 0x272e38)
    static let surface = Color(hex: 0x2b323d)
    static let raised = Color(hex: 0x2e3541)
    static let chip = Color(hex: 0x343c48)
    static let field = Color(hex: 0x1c2129)
    static let line = Color(hex: 0x363e4a)
    static let fieldEdge = Color(hex: 0x424b58)
    static let edge = Color(hex: 0x46505e)
    static let selected = Color(hex: 0x344155)
    static let button = Color(hex: 0x2f3641)

    static let text = Color(hex: 0xe7ebf0)
    static let soft = Color(hex: 0xc9d0d9)
    static let label = Color(hex: 0xb3bcc8)
    static let muted = Color(hex: 0xa0aab7)

    static let accent = Color(hex: 0x4cc4f9)
    static let onAccent = Color(hex: 0x04121c)

    static let ok = Color(hex: 0x22c55e)
    static let amber = Color(hex: 0xf59e0b)
    static let lost = Color(hex: 0xf97316)
    static let idle = Color(hex: 0x7b8595)
    static let bad = Color(hex: 0xf87171)
    /// Behind white text that deletes (a swipe action): dark enough to read.
    static let danger = Color(hex: 0xdc2626)
    static let badBg = Color(hex: 0x3a1a1f)
    static let badEdge = Color(hex: 0x8a2a2a)
    static let badText = Color(hex: 0xfecaca)
}

extension Color {
    /// `0xrrggbb`.
    init(hex: UInt32) {
        self.init(
            red: Double((hex >> 16) & 0xff) / 255,
            green: Double((hex >> 8) & 0xff) / 255,
            blue: Double(hex & 0xff) / 255
        )
    }
}

extension UIColor {
    /// `0xrrggbb`.
    convenience init(hex: UInt32) {
        self.init(
            red: CGFloat((hex >> 16) & 0xff) / 255,
            green: CGFloat((hex >> 8) & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255,
            alpha: 1
        )
    }
}

/// A section's heading: small capitals in the muted color, as the desktop's.
struct SectionHeading: View {
    let text: String
    var body: some View {
        Text(text.uppercased())
            .font(.system(size: 12, weight: .semibold))
            .tracking(0.7)
            .foregroundStyle(Dusk.muted)
    }
}
