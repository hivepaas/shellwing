import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var app: AppModel
    @EnvironmentObject private var lock: AppLock

    private let choices = [0, 1, 5, 15, 30, 60]
    private let scrollbacks: [UInt64] = [1_000, 5_000, 10_000, 50_000, 100_000]
    private let keepalives: [UInt64] = [0, 15, 30, 60, 120]
    @State private var settings: SettingsRow?
    @AppStorage(TerminalCanvas.fontSizeKey) private var fontSize = TerminalCanvas.defaultFontSize

    private var themeName: String {
        guard let id = settings?.theme else { return "" }
        return app.core?.themes().first { $0.id == id }?.name ?? id
    }

    /// Changes a setting, then reads them all again.
    private func change(_ set: (Shellwing) throws -> Void) {
        guard let core = app.core else { return }
        do {
            try set(core)
        } catch {
            app.error = "\(error)"
        }
        settings = core.settings()
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Picker("Lock shellwing when away", selection: $lock.after) {
                        ForEach(choices, id: \.self) { m in
                            Text(m == 0 ? "Off" : m == 60 ? "1 hour" : "\(m) min").tag(m)
                        }
                    }
                    .disabled(!lock.available)
                    if lock.after > 0 {
                        Button("Lock now") { lock.lockNow() }
                    }
                } header: {
                    SectionHeading(text: "Lock")
                } footer: {
                    Text(lock.available
                        ? "When shellwing starts, and after this long in the background, it hides until \(lock.biometry) or your passcode says it is you. A light guard for a phone left unlocked: the sessions go on underneath."
                        : "This phone has no passcode, so nothing could ask that it is you.")
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)

                Section {
                    NavigationLink {
                        ThemePicker(selected: settings?.theme, appDefault: false) { id in
                            if let id { change { try $0.setTheme(id: id) } }
                        }
                    } label: {
                        LabeledContent("Theme") {
                            Text(themeName).foregroundStyle(Dusk.muted)
                        }
                    }
                    Stepper(value: $fontSize, in: TerminalCanvas.fontSizes, step: 0.5) {
                        LabeledContent("Text size", value: String(format: "%g pt", fontSize))
                    }
                    Picker("History", selection: Binding(
                        get: { settings?.scrollback ?? 10_000 },
                        set: { v in change { try $0.setScrollback(lines: v) } }
                    )) {
                        ForEach(scrollbacks, id: \.self) { n in
                            Text("\(n.formatted()) lines").tag(n)
                        }
                    }
                } header: {
                    SectionHeading(text: "Terminal")
                } footer: {
                    Text("Sessions opened after a change take it. A pinch on the terminal sizes its text too; the size is this phone's own.")
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)

                Section {
                    Picker("New host keys", selection: Binding(
                        get: { settings?.hostKey ?? .ask },
                        set: { v in change { try $0.setHostKey(way: v) } }
                    )) {
                        Text("Ask").tag(HostKeyWay.ask)
                        Text("Accept them").tag(HostKeyWay.acceptNew)
                        Text("Refuse them").tag(HostKeyWay.strict)
                    }
                    Picker("Keepalive", selection: Binding(
                        get: { settings?.keepalive ?? 30 },
                        set: { v in change { try $0.setKeepalive(seconds: v) } }
                    )) {
                        ForEach(keepalives, id: \.self) { s in
                            Text(s == 0 ? "None" : "Every \(s) s").tag(s)
                        }
                    }
                    NavigationLink("Known hosts") { KnownHostsView() }
                } header: {
                    SectionHeading(text: "Connections")
                } footer: {
                    Text("A host's own, in its editor, wins. A key that changed is always refused: forget the host in Known hosts to accept its new one.")
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)

                Section {
                    let state = app.vaultState()
                    if state.ready {
                        NavigationLink {
                            SyncView()
                        } label: {
                            LabeledContent("Sync") {
                                Text(app.sync?.said ?? "Off").foregroundStyle(Dusk.muted)
                            }
                        }
                        NavigationLink("SSH keys") { KeysView() }
                        NavigationLink("Saved passwords") { SecretsView() }
                        NavigationLink("Recovery key") { RecoveryKeyView() }
                    } else {
                        Text(state.problem ?? "The vault is opening…")
                            .foregroundStyle(Dusk.badText)
                    }
                } header: {
                    SectionHeading(text: "Vault")
                } footer: {
                    Text("Passwords and keys, encrypted on this phone; its key is in the iOS Keychain, on this phone only.")
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)

                Section {
                    LabeledContent("Hosts", value: "\(app.sections.flatMap(\.hosts).count)")
                    LabeledContent("Configuration") {
                        Text("hosts.toml, in the app's own folder")
                            .foregroundStyle(Dusk.muted)
                            .font(.system(size: 14))
                    }
                } header: {
                    SectionHeading(text: "Hosts")
                }
                .listRowBackground(Dusk.surface)

                Section {
                    LabeledContent("Version", value: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "")
                    Link("shellwing on GitHub", destination: URL(string: "https://github.com/hivepaas/shellwing")!)
                } header: {
                    SectionHeading(text: "About")
                }
                .listRowBackground(Dusk.surface)

                if let problem = app.problem {
                    Section {
                        Text(problem).foregroundStyle(Dusk.badText)
                    } header: {
                        SectionHeading(text: "Problem")
                    }
                    .listRowBackground(Dusk.badBg)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.panel.ignoresSafeArea())
            .navigationTitle("Settings")
            .onAppear { settings = app.core?.settings() }
            .onChange(of: app.sync?.last) { _, _ in settings = app.core?.settings() }
        }
    }
}

/// The terminal's themes, each as a swatch of its colors.
struct ThemePicker: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    let selected: String?
    /// "Same as the app" is a choice (a host's theme): nil.
    let appDefault: Bool
    let chose: (String?) -> Void

    var body: some View {
        List {
            if appDefault {
                row(name: "Same as the app", on: selected == nil) { pick(nil) }
            }
            ForEach(app.core?.themes() ?? [], id: \.id) { t in
                Button { pick(t.id) } label: {
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text("~ $ ls").font(.system(size: 12, design: .monospaced)).foregroundStyle(Color(hex: t.foreground))
                            HStack(spacing: 2) {
                                ForEach(Array(t.palette.enumerated()), id: \.offset) { _, c in
                                    RoundedRectangle(cornerRadius: 2).fill(Color(hex: c)).frame(width: 8, height: 8)
                                }
                            }
                        }
                        .padding(8)
                        .background(Color(hex: t.background), in: RoundedRectangle(cornerRadius: 6))
                        .overlay(RoundedRectangle(cornerRadius: 6).stroke(Dusk.line))
                        Text(t.name).foregroundStyle(Dusk.text)
                        Spacer()
                        if selected == t.id { Image(systemName: "checkmark").foregroundStyle(Dusk.accent) }
                    }
                }
                .listRowBackground(Dusk.surface)
            }
        }
        .scrollContentBackground(.hidden)
        .background(Dusk.page.ignoresSafeArea())
        .navigationTitle("Theme")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func row(name: String, on: Bool, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack {
                Text(name).foregroundStyle(Dusk.text)
                Spacer()
                if on { Image(systemName: "checkmark").foregroundStyle(Dusk.accent) }
            }
        }
        .listRowBackground(Dusk.surface)
    }

    private func pick(_ id: String?) {
        chose(id)
        dismiss()
    }
}

/// The hosts whose keys are known; one forgotten has its key asked about
/// again next time.
struct KnownHostsView: View {
    @EnvironmentObject private var app: AppModel
    @State private var rows: [KnownHostRow] = []
    @State private var problem: String?
    @State private var filter = ""

    var body: some View {
        List {
            if let problem {
                Text(problem).foregroundStyle(Dusk.badText).listRowBackground(Dusk.badBg)
            }
            if rows.isEmpty && problem == nil {
                Text("No host keys known yet: a host's is kept the first time you connect.")
                    .foregroundStyle(Dusk.muted)
                    .listRowBackground(Dusk.surface)
            }
            ForEach(shown, id: \.line) { r in
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        Text(r.names.isEmpty ? "A hashed name" : r.names.joined(separator: ", "))
                            .font(.system(size: 15, weight: .medium))
                            .foregroundStyle(r.names.isEmpty ? Dusk.muted : Dusk.text)
                        if let m = r.marker {
                            Text(m).font(.system(size: 11)).foregroundStyle(Dusk.amber)
                        }
                    }
                    Text("\(r.kind) · \(r.fingerprint)")
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundStyle(Dusk.muted)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                .listRowBackground(Dusk.surface)
                .swipeActions {
                    Button("Forget", role: .destructive) { forget([r.line]) }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .background(Dusk.page.ignoresSafeArea())
        .searchable(text: $filter, prompt: "Filter by name")
        .navigationTitle("Known hosts")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear(perform: reload)
    }

    private var shown: [KnownHostRow] {
        let q = filter.lowercased()
        guard !q.isEmpty else { return rows }
        return rows.filter { $0.names.contains { $0.lowercased().contains(q) } }
    }

    private func reload() {
        do {
            rows = try app.core?.knownHosts() ?? []
            problem = nil
        } catch {
            problem = "\(error)"
        }
    }

    private func forget(_ lines: [UInt64]) {
        do {
            _ = try app.core?.forgetKnownHosts(lines: lines)
        } catch {
            problem = "\(error)"
        }
        reload()
    }
}
