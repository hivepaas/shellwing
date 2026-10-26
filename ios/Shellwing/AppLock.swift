import LocalAuthentication
import SwiftUI

/// shellwing locked while its user is away, as on the desktop: when it starts
/// and after a while in the background, Face ID or the passcode opens it.
/// A light guard for a phone left unlocked; the sessions go on underneath.
@MainActor
final class AppLock: ObservableObject {
    /// Minutes in the background before it locks; 0: never (the default).
    @AppStorage("lock.after") var after = 0 {
        willSet { objectWillChange.send() }
    }
    @Published private(set) var locked = false
    @Published private(set) var said: String?
    private var away: Date?

    init() {
        locked = UserDefaults.standard.integer(forKey: "lock.after") > 0
    }

    func wentAway() {
        away = Date()
    }

    func cameBack() {
        guard after > 0, let away, !locked else { return }
        if Date().timeIntervalSince(away) >= Double(after * 60) {
            locked = true
            said = nil
        }
        self.away = nil
    }

    func lockNow() {
        locked = true
        said = nil
    }

    func unlock() {
        let ctx = LAContext()
        var error: NSError?
        guard ctx.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) else {
            // No passcode on this phone: nothing to ask, nothing to guard.
            locked = false
            return
        }
        ctx.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: tr("Unlock shellwing")) { ok, err in
            Task { @MainActor in
                if ok {
                    self.locked = false
                    self.said = nil
                } else if let e = err as? LAError, e.code == .userCancel || e.code == .appCancel || e.code == .systemCancel {
                    self.said = tr("Not confirmed: shellwing stays locked.")
                } else {
                    self.said = err?.localizedDescription ?? tr("Not confirmed: shellwing stays locked.")
                }
            }
        }
    }

    /// Whether this phone can ask at all (a passcode is set).
    var available: Bool {
        LAContext().canEvaluatePolicy(.deviceOwnerAuthentication, error: nil)
    }

    /// Face ID, Touch ID or Optic ID; nil when only the passcode asks.
    var biometry: String? {
        let ctx = LAContext()
        _ = ctx.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: nil)
        switch ctx.biometryType {
        case .faceID: return "Face ID"
        case .touchID: return "Touch ID"
        case .opticID: return "Optic ID"
        default: return nil
        }
    }
}

struct LockView: View {
    @ObservedObject var lock: AppLock
    let sessions: Int

    var body: some View {
        VStack(spacing: 16) {
            Spacer()
            RoundedRectangle(cornerRadius: 24)
                .fill(Dusk.surface)
                .frame(width: 84, height: 84)
                .overlay(RoundedRectangle(cornerRadius: 24).stroke(Dusk.edge))
                .overlay(Wing().fill(Dusk.accent).frame(width: 48, height: 48))
            Text(tr("shellwing is locked")).font(.system(size: 24, weight: .semibold))
            Text(lock.biometry.map { tr("{biometry} or your passcode unlocks it.", ["biometry": $0]) }
                ?? tr("Your passcode unlocks it."))
                .font(.system(size: 16))
                .foregroundStyle(Dusk.label)
                .multilineTextAlignment(.center)
            if let said = lock.said {
                Text(said)
                    .font(.system(size: 14))
                    .foregroundStyle(Dusk.badText)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(Dusk.badBg, in: RoundedRectangle(cornerRadius: 10))
                    .overlay(RoundedRectangle(cornerRadius: 10).stroke(Dusk.badEdge))
            }
            Spacer()
            if sessions > 0 {
                Text(trN(sessions, "{n} session stays connected underneath.", "{n} sessions stay connected underneath."))
                    .font(.system(size: 13))
                    .foregroundStyle(Dusk.muted)
            }
            Button {
                lock.unlock()
            } label: {
                Label(tr("Unlock"), systemImage: "faceid")
                    .font(.system(size: 17, weight: .semibold))
                    .frame(maxWidth: .infinity, minHeight: 54)
            }
            .background(Dusk.accent, in: RoundedRectangle(cornerRadius: 16))
            .foregroundStyle(Dusk.onAccent)
        }
        .padding(.horizontal, 24)
        .padding(.bottom, 12)
        .foregroundStyle(Dusk.text)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Dusk.page)
        .onAppear { lock.unlock() }
    }
}

/// The logo: the wing (crates/app/assets/icons/logo.svg), in a 24-point box.
struct Wing: Shape {
    func path(in rect: CGRect) -> Path {
        let s = min(rect.width, rect.height) / 24
        func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: rect.minX + x * s, y: rect.minY + y * s) }
        var path = Path()
        path.move(to: p(3, 17))
        path.addCurve(to: p(12, 8), control1: p(7, 16), control2: p(10, 13))
        path.addCurve(to: p(21, 16), control1: p(13, 12), control2: p(16, 15))
        path.addCurve(to: p(12, 21), control1: p(16, 17), control2: p(13, 18))
        path.addCurve(to: p(3, 17), control1: p(11, 18), control2: p(8, 17))
        path.closeSubpath()
        return path
    }
}
