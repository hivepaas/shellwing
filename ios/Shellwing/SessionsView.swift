import SwiftUI

/// Every open session, the desktop's tab overview: where each one stands.
struct SessionsView: View {
    @EnvironmentObject private var app: AppModel
    @State private var path: [SessionModel] = []

    private let columns = [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)]

    var body: some View {
        NavigationStack(path: $path) {
            ScrollView {
                if app.sessions.isEmpty {
                    VStack(spacing: 10) {
                        Image(systemName: "terminal").font(.system(size: 34)).foregroundStyle(Dusk.muted)
                        Text("No sessions open").font(.headline).foregroundStyle(Dusk.text)
                        Text("Open one from Hosts.").font(.subheadline).foregroundStyle(Dusk.muted)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.top, 80)
                } else {
                    LazyVGrid(columns: columns, spacing: 12) {
                        ForEach(app.sessions) { s in
                            SessionCard(model: s, open: { path = [s] }, close: { app.close(s) })
                        }
                    }
                    .padding(16)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Dusk.panel.ignoresSafeArea())
            .navigationTitle("Sessions")
            .navigationDestination(for: SessionModel.self) { s in
                TerminalScreen(model: s)
            }
        }
    }
}

struct SessionCard: View {
    @ObservedObject var model: SessionModel
    let open: () -> Void
    let close: () -> Void

    var body: some View {
        Button(action: open) {
            VStack(spacing: 0) {
                preview
                    .frame(height: 120)
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                    .background(Color(hex: model.frame?.background ?? 0x272e38))
                    .clipped()
                HStack(spacing: 8) {
                    Circle().fill(model.status.color).frame(width: 8, height: 8)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(model.spec).font(.system(size: 14, weight: .semibold)).foregroundStyle(Dusk.text)
                        Text(model.status.said).font(.system(size: 12)).foregroundStyle(Dusk.muted)
                    }
                    Spacer(minLength: 0)
                }
                .padding(10)
            }
            .background(Dusk.surface)
            .clipShape(RoundedRectangle(cornerRadius: 14))
            .overlay(RoundedRectangle(cornerRadius: 14).stroke(Dusk.edge, lineWidth: 1))
        }
        .contextMenu {
            if model.status != .connected && model.status != .connecting {
                Button("Connect again", systemImage: "arrow.clockwise") { model.reconnect() }
            }
            Button("Close", systemImage: "xmark", role: .destructive, action: close)
        }
    }

    /// The last rows with text, small.
    private var preview: some View {
        let rows = (model.frame?.rows ?? [])
            .map { $0.runs.map(\.text).joined() }
            .filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
            .suffix(10)
        return Text(rows.joined(separator: "\n"))
            .font(.system(size: 7.5, design: .monospaced))
            .foregroundStyle(Color(hex: model.frame?.foreground ?? 0xe2e8f0))
            .lineLimit(10)
            .multilineTextAlignment(.leading)
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .padding(8)
    }
}
