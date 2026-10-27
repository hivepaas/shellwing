import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var app: AppModel
    @EnvironmentObject private var lock: AppLock

    private let choices = [0, 1, 5, 15, 30, 60]
    private let scrollbacks: [UInt64] = [1_000, 5_000, 10_000, 50_000, 100_000]
    private let keepalives: [UInt64] = [0, 15, 30, 60, 120]
    @State private var settings: SettingsRow?
    @State private var debugScreen = UserDefaults.standard.string(forKey: "screen")
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
                    Picker(tr("Lock shellwing when away"), selection: $lock.after) {
                        ForEach(choices, id: \.self) { m in
                            Text(m == 0 ? tr("Off") : m == 60 ? tr("1 hour") : trN(m, "{n} minute", "{n} minutes")).tag(m)
                        }
                    }
                    .disabled(!lock.available)
                    if lock.after > 0 {
                        Button(tr("Lock now")) { lock.lockNow() }
                    }
                } header: {
                    SectionHeading(text: tr("Lock"))
                } footer: {
                    Text(lock.available
                        ? (lock.biometry.map {
                            tr("When shellwing starts, and after this long in the background, it hides until {biometry} or your passcode says it is you. A light guard for a phone left unlocked: the sessions go on underneath.", ["biometry": $0])
                        } ?? tr("When shellwing starts, and after this long in the background, it hides until your passcode says it is you. A light guard for a phone left unlocked: the sessions go on underneath."))
                        : tr("This phone has no passcode, so nothing could ask that it is you."))
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)

                Section {
                    Picker(tr("Language"), selection: Binding(
                        get: { settings?.language ?? "" },
                        set: { v in
                            app.setLanguage(v.isEmpty ? nil : v)
                            settings = app.core?.settings()
                        }
                    )) {
                        Text(tr("As the phone")).tag("")
                        ForEach(languages(), id: \.tag) { l in Text(l.name).tag(l.tag) }
                    }
                } header: {
                    SectionHeading(text: tr("Language"))
                }
                .listRowBackground(Dusk.surface)

                Section {
                    NavigationLink {
                        ThemePicker(selected: settings?.theme, appDefault: false) { id in
                            if let id { change { try $0.setTheme(id: id) } }
                        }
                    } label: {
                        LabeledContent(tr("Theme")) {
                            Text(themeName).foregroundStyle(Dusk.muted)
                        }
                    }
                    Stepper(value: $fontSize, in: TerminalCanvas.fontSizes, step: 0.5) {
                        LabeledContent(tr("Text size"), value: String(format: "%g pt", fontSize))
                    }
                    Picker(tr("History"), selection: Binding(
                        get: { settings?.scrollback ?? 10_000 },
                        set: { v in change { try $0.setScrollback(lines: v) } }
                    )) {
                        ForEach(scrollbacks, id: \.self) { n in
                            Text(trN(Int(n), "{n} line", "{n} lines")).tag(n)
                        }
                    }
                } header: {
                    SectionHeading(text: tr("Terminal"))
                } footer: {
                    Text(tr("Sessions opened after a change take it. A pinch on the terminal sizes its text too; the size is this phone's own."))
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)

                Section {
                    Picker(tr("New host keys"), selection: Binding(
                        get: { settings?.hostKey ?? .ask },
                        set: { v in change { try $0.setHostKey(way: v) } }
                    )) {
                        Text(tr("Ask")).tag(HostKeyWay.ask)
                        Text(tr("Accept them")).tag(HostKeyWay.acceptNew)
                        Text(tr("Refuse them")).tag(HostKeyWay.strict)
                    }
                    Picker(tr("Keepalive"), selection: Binding(
                        get: { settings?.keepalive ?? 30 },
                        set: { v in change { try $0.setKeepalive(seconds: v) } }
                    )) {
                        ForEach(keepalives, id: \.self) { s in
                            Text(s == 0 ? tr("None") : trN(Int(s), "Every {n} second", "Every {n} seconds")).tag(s)
                        }
                    }
                    Picker(tr("When a connection drops"), selection: Binding(
                        get: { settings?.onDisconnect ?? .reconnect },
                        set: { v in change { try $0.setOnDisconnect(way: v) } }
                    )) {
                        Text(tr("Reconnect")).tag(OnDisconnectWay.reconnect)
                        Text(tr("Ask")).tag(OnDisconnectWay.ask)
                        Text(tr("Close")).tag(OnDisconnectWay.close)
                    }
                    NavigationLink(tr("Known hosts")) { KnownHostsView() }
                } header: {
                    SectionHeading(text: tr("Connections"))
                } footer: {
                    Text(tr("A host's own, in its editor, wins. A key that changed is always refused: forget the host in Known hosts to accept its new one.")
                        + "\n\n" + tr("iOS stops an app soon after it leaves the screen, and its connections with it. Reconnect makes a lost one again by itself, waiting longer each time, and at once when you come back; a host's shell kept in tmux (in its editor) is found as it was."))
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)

                Section {
                    let state = app.vaultState()
                    if state.ready {
                        NavigationLink {
                            SyncView()
                        } label: {
                            LabeledContent(tr("Sync")) {
                                Text(app.sync?.said ?? tr("Off")).foregroundStyle(Dusk.muted)
                            }
                        }
                        NavigationLink(tr("SSH keys")) { KeysView() }
                        NavigationLink(tr("Saved passwords")) { SecretsView() }
                        NavigationLink(tr("Recovery key")) { RecoveryKeyView() }
                    } else {
                        Text(state.problem ?? tr("The vault is opening…"))
                            .foregroundStyle(Dusk.badText)
                    }
                } header: {
                    SectionHeading(text: tr("Vault"))
                } footer: {
                    Text(tr("Passwords and keys, encrypted on this phone; its key is in the iOS Keychain, on this phone only."))
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)

                Section {
                    LabeledContent(tr("Hosts"), value: "\(app.sections.flatMap(\.hosts).count)")
                    LabeledContent(tr("Configuration")) {
                        Text(tr("hosts.toml, in the app's own folder"))
                            .foregroundStyle(Dusk.muted)
                            .font(.system(size: 14))
                    }
                } header: {
                    SectionHeading(text: tr("Hosts"))
                }
                .listRowBackground(Dusk.surface)

                Section {
                    LabeledContent(tr("Version"), value: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "")
                    Link(tr("shellwing on GitHub"), destination: URL(string: "https://github.com/hivepaas/shellwing")!)
                } header: {
                    SectionHeading(text: tr("About"))
                }
                .listRowBackground(Dusk.surface)

                if let problem = app.problem {
                    Section {
                        Text(problem).foregroundStyle(Dusk.badText)
                    } header: {
                        SectionHeading(text: tr("Problem"))
                    }
                    .listRowBackground(Dusk.badBg)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.panel.ignoresSafeArea())
            .navigationTitle(tr("Settings"))
            #if DEBUG
            // `-screen sync` at launch (trying it in the Simulator): Sync.
            .navigationDestination(isPresented: Binding(
                get: { debugScreen == "sync" },
                set: { if !$0 { debugScreen = nil } }
            )) { SyncView() }
            #endif
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
                row(name: tr("Same as the app"), on: selected == nil) { pick(nil) }
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
        .navigationTitle(tr("Theme"))
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
    @State private var doomed: Doomed?
    @State private var problem: String?
    @State private var filter = ""

    var body: some View {
        List {
            if let problem {
                Text(problem).foregroundStyle(Dusk.badText).listRowBackground(Dusk.badBg)
            }
            if rows.isEmpty && problem == nil {
                Text(tr("No host keys known yet: a host's is kept the first time you connect."))
                    .foregroundStyle(Dusk.muted)
                    .listRowBackground(Dusk.surface)
            }
            ForEach(shown, id: \.line) { r in
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        Text(r.names.isEmpty ? tr("A hashed name") : r.names.joined(separator: ", "))
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
                .confirming($doomed, about: "\(r.line)")
                .swipeActions {
                    DeleteSwipe(label: tr("Forget")) {
                        doomed = Doomed(
                            key: "\(r.line)",
                            title: tr("Forget the key of {host}?", ["host": r.names.isEmpty ? tr("A hashed name") : r.names.joined(separator: ", ")]),
                            message: tr("It is shown to check again the next time you connect."),
                            action: tr("Forget")
                        ) { forget([r.line]) }
                    }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .background(Dusk.page.ignoresSafeArea())
        .searchable(text: $filter, prompt: tr("Filter by name"))
        .navigationTitle(tr("Known hosts"))
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
