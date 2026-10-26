import SwiftUI

/// A session full screen: its host and status on top, the terminal, and the
/// keys a phone's keyboard lacks under it.
struct TerminalScreen: View {
    @ObservedObject var model: SessionModel
    @Environment(\.dismiss) private var dismiss
    @State private var typing = true

    var body: some View {
        VStack(spacing: 0) {
            header
            TerminalView(model: model, typing: $typing)
                .overlay(alignment: .bottom) { banner }
            KeysRow(model: model, typing: $typing)
        }
        .background(Dusk.chrome)
        .toolbar(.hidden, for: .navigationBar)
        .toolbar(.hidden, for: .tabBar)
    }

    private var header: some View {
        HStack(spacing: 4) {
            Button { dismiss() } label: {
                Image(systemName: "chevron.left")
                    .font(.system(size: 19, weight: .semibold))
                    .frame(width: 44, height: 44)
            }
            .foregroundStyle(Dusk.accent)
            .accessibilityLabel("Hosts")
            VStack(spacing: 1) {
                HStack(spacing: 7) {
                    Circle().fill(model.status.color).frame(width: 8, height: 8)
                    Text(model.spec).font(.system(size: 16, weight: .semibold))
                }
                Text(model.title == model.spec ? model.status.said : model.title)
                    .font(.system(size: 12))
                    .foregroundStyle(Dusk.muted)
                    .lineLimit(1)
            }
            .frame(maxWidth: .infinity)
            Menu {
                Button("Paste", systemImage: "doc.on.clipboard") {
                    if let s = UIPasteboard.general.string { model.paste(s) }
                }
                Button("Back to the bottom", systemImage: "arrow.down.to.line") {
                    model.scrollToBottom()
                }
                if model.status == .lost || model.status == .closed || model.status == .failed {
                    Button("Connect again", systemImage: "arrow.clockwise") { model.reconnect() }
                }
            } label: {
                Image(systemName: "ellipsis")
                    .font(.system(size: 18, weight: .semibold))
                    .frame(width: 44, height: 44)
            }
            .foregroundStyle(Dusk.soft)
            .accessibilityLabel("Actions")
        }
        .padding(.horizontal, 6)
        .frame(height: 52)
        .foregroundStyle(Dusk.text)
        .background(Dusk.chrome)
        .overlay(alignment: .bottom) { Dusk.line.frame(height: 1) }
    }

    /// How the connection ended, and a way to connect again.
    @ViewBuilder
    private var banner: some View {
        if model.status == .lost || model.status == .closed || model.status == .failed {
            HStack(spacing: 10) {
                Text(model.ended ?? model.status.said)
                    .font(.system(size: 13))
                    .foregroundStyle(model.status == .failed ? Dusk.badText : Dusk.soft)
                    .lineLimit(2)
                Spacer(minLength: 8)
                Button("Connect again") { model.reconnect() }
                    .font(.system(size: 14, weight: .semibold))
                    .padding(.horizontal, 12)
                    .frame(height: 34)
                    .background(Dusk.accent, in: Capsule())
                    .foregroundStyle(Dusk.onAccent)
            }
            .padding(12)
            .background(Dusk.field.opacity(0.95))
        }
    }
}

/// The keys a phone's keyboard lacks: Esc, Tab, Ctrl and Alt (held for the
/// next key), the arrows, and the symbols shells use most.
struct KeysRow: View {
    @ObservedObject var model: SessionModel
    @Binding var typing: Bool

    var body: some View {
        VStack(spacing: 5) {
            HStack(spacing: 5) {
                key("Esc") { model.press(.escape) }
                key("Tab") { model.press(.tab) }
                toggle("Ctrl", on: model.ctrl) { model.ctrl.toggle() }
                toggle("Alt", on: model.alt) { model.alt.toggle() }
                icon("chevron.left", "Left") { model.press(.left) }
                icon("chevron.up", "Up") { model.press(.up) }
                icon("chevron.down", "Down") { model.press(.down) }
                icon("chevron.right", "Right") { model.press(.right) }
            }
            HStack(spacing: 5) {
                ForEach(["|", "~", "/", "-", "_", ":"], id: \.self) { s in
                    key(s, mono: true) { model.type(s) }
                }
                Button {
                    typing.toggle()
                } label: {
                    Image(systemName: typing ? "keyboard.chevron.compact.down" : "keyboard")
                        .frame(maxWidth: .infinity, minHeight: 40)
                }
                .background(Dusk.accent, in: RoundedRectangle(cornerRadius: 9))
                .foregroundStyle(Dusk.onAccent)
                .accessibilityLabel(typing ? "Hide the keyboard" : "Show the keyboard")
            }
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 8)
        .background(Dusk.chrome)
        .overlay(alignment: .top) { Dusk.line.frame(height: 1) }
    }

    private func key(_ label: String, mono: Bool = false, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(label)
                .font(mono ? .system(size: 15, design: .monospaced) : .system(size: 14, weight: .medium))
                .frame(maxWidth: .infinity, minHeight: 40)
        }
        .background(Dusk.button, in: RoundedRectangle(cornerRadius: 9))
        .foregroundStyle(Dusk.text)
    }

    private func toggle(_ label: String, on: Bool, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(label)
                .font(.system(size: 14, weight: on ? .bold : .medium))
                .frame(maxWidth: .infinity, minHeight: 40)
        }
        .background(on ? Dusk.selected : Dusk.button, in: RoundedRectangle(cornerRadius: 9))
        .overlay(RoundedRectangle(cornerRadius: 9).stroke(on ? Dusk.accent : .clear, lineWidth: 1))
        .foregroundStyle(Dusk.text)
        .accessibilityAddTraits(on ? .isSelected : [])
    }

    private func icon(_ name: String, _ label: String, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: name)
                .font(.system(size: 14, weight: .semibold))
                .frame(maxWidth: .infinity, minHeight: 40)
        }
        .background(Dusk.button, in: RoundedRectangle(cornerRadius: 9))
        .foregroundStyle(Dusk.text)
        .accessibilityLabel(label)
    }
}
