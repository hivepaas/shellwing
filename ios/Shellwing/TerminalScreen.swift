import SwiftUI

/// A session full screen: its host and status on top, the terminal, and the
/// keys a phone's keyboard lacks under it.
struct TerminalScreen: View {
    @ObservedObject var model: SessionModel
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var typing = true
    @State private var installing = false
    @State private var pickingSnippet = false
    @State private var running: SnippetRow?
    @State private var pickingEnv = false
    @State private var browsing = false
    @State private var placing = false
    /// What the last thing done said: a key installed, a password kept.
    @State private var said: String?

    var body: some View {
        VStack(spacing: 0) {
            header
            if !model.envs.isEmpty || !model.placed.isEmpty { badges }
            TerminalView(model: model, typing: $typing)
                .overlay(alignment: .bottom) {
                    VStack(spacing: 0) {
                        if let said { note(said) }
                        if let offer = model.offer { offerBar(offer) }
                        banner
                    }
                }
            KeysRow(model: model, typing: $typing) { pickingSnippet = true }
        }
        .background(Dusk.chrome)
        .toolbar(.hidden, for: .navigationBar)
        .toolbar(.hidden, for: .tabBar)
        .sheet(isPresented: $pickingSnippet) {
            NavigationStack {
                SnippetsList(spec: model.spec) { s in
                    pickingSnippet = false
                    if s.asked.isEmpty {
                        if let why = app.run(snippet: s.id, in: model, values: [:]) { show(why) }
                    } else {
                        running = s
                    }
                }
                .background(Dusk.panel.ignoresSafeArea())
                .navigationTitle("Snippets")
                .navigationBarTitleDisplayMode(.inline)
            }
            .environmentObject(app)
            .presentationDetents([.medium, .large])
            .preferredColorScheme(.dark)
        }
        .sheet(item: $running) { s in
            RunSnippetSheet(snippet: s, session: model).environmentObject(app)
        }
        .sheet(isPresented: $pickingEnv) {
            NavigationStack {
                List(app.environments(), id: \.id) { e in
                    Button {
                        pickingEnv = false
                        if let why = app.load(env: e.id, in: model) { show(why) }
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(e.name).foregroundStyle(Dusk.text)
                            Text(e.names.joined(separator: " "))
                                .font(.system(size: 12, design: .monospaced))
                                .foregroundStyle(Dusk.muted)
                        }
                    }
                    .listRowBackground(Dusk.surface)
                }
                .scrollContentBackground(.hidden)
                .background(Dusk.panel.ignoresSafeArea())
                .navigationTitle("Load an environment")
                .navigationBarTitleDisplayMode(.inline)
            }
            .presentationDetents([.medium])
            .preferredColorScheme(.dark)
        }
        .onChange(of: model.notice) { _, n in
            if let n {
                show(n)
                model.notice = nil
            }
        }
        .sheet(isPresented: $browsing) {
            FilesScreen(model: model).environmentObject(app)
        }
        .sheet(isPresented: $placing) {
            PlaceConfigSheet(model: model) { result in
                placing = false
                if let result { show(result) }
            }
            .environmentObject(app)
            .presentationDetents([.medium, .large])
        }
        .sheet(isPresented: $installing) {
            InstallKeySheet(model: model) { result in
                installing = false
                if let result { show(result) }
            }
            .environmentObject(app)
            .presentationDetents([.medium, .large])
        }
    }

    /// The environments loaded into the shell (✕ unloads one), and the config
    /// files placed until the session ends (kept, or taken away, from its menu).
    private var badges: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(model.placed, id: \.path) { p in
                    Menu {
                        Text(p.path)
                        Button("Keep it there", systemImage: "pin") { placed(keep: true, p.path) }
                        Button("Take it away now", systemImage: "xmark", role: .destructive) { placed(keep: false, p.path) }
                    } label: {
                        HStack(spacing: 5) {
                            Image(systemName: "doc.badge.clock").font(.system(size: 12)).foregroundStyle(Dusk.amber)
                            Text(p.name).font(.system(size: 13))
                        }
                        .padding(.horizontal, 10)
                        .frame(height: 30)
                        .background(Dusk.chip, in: Capsule())
                        .overlay(Capsule().stroke(Dusk.fieldEdge))
                    }
                    .accessibilityLabel("\(p.name), placed until the session ends")
                }
                ForEach(model.envs, id: \.id) { e in
                    HStack(spacing: 4) {
                        Circle().fill(Dusk.ok).frame(width: 6, height: 6)
                        Text(e.name).font(.system(size: 13))
                        Button { model.unload(env: e.id) } label: {
                            Image(systemName: "xmark").font(.system(size: 11, weight: .semibold))
                                .frame(width: 24, height: 24)
                        }
                        .accessibilityLabel("Unload \(e.name)")
                    }
                    .padding(.leading, 10)
                    .frame(height: 30)
                    .background(Dusk.chip, in: Capsule())
                    .overlay(Capsule().stroke(Dusk.fieldEdge))
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
        }
        .foregroundStyle(Dusk.text)
        .background(Dusk.chrome)
    }

    /// Keeps the file placed at `path` for good, or takes it away now.
    private func placed(keep: Bool, _ path: String) {
        guard let core = app.core, let h = model.handle else { return }
        Task {
            do {
                if keep {
                    try await core.keepPlaced(session: h, path: path)
                    show("\(path) stays when the session ends.")
                } else {
                    try await core.removePlaced(session: h, path: path)
                    show("\(path) taken away.")
                }
            } catch {
                show("\(error)")
            }
            model.refreshPlaced()
        }
    }

    private func show(_ text: String) {
        said = text
        Task {
            try? await Task.sleep(for: .seconds(4))
            if said == text { said = nil }
        }
    }

    private func note(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 13))
            .foregroundStyle(Dusk.text)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
            .background(Dusk.raised.opacity(0.97))
    }

    /// A password that worked, offered to keep in the vault.
    private func offerBar(_ offer: String) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Save \(offer) in the vault? It is tried first next time.")
                .font(.system(size: 13))
                .foregroundStyle(Dusk.text)
            HStack(spacing: 8) {
                Spacer()
                Button("Not now") { model.dismissOffer() }
                    .font(.system(size: 14))
                    .padding(.horizontal, 12)
                    .frame(height: 34)
                    .background(Dusk.button, in: Capsule())
                    .foregroundStyle(Dusk.text)
                Button("Save") {
                    do {
                        try model.keepOffer()
                        show("Saved in the vault.")
                    } catch {
                        show("Not saved: \(error)")
                    }
                }
                .font(.system(size: 14, weight: .semibold))
                .padding(.horizontal, 14)
                .frame(height: 34)
                .background(Dusk.accent, in: Capsule())
                .foregroundStyle(Dusk.onAccent)
            }
        }
        .padding(12)
        .background(Dusk.raised.opacity(0.97))
        .overlay(alignment: .top) { Dusk.edge.frame(height: 1) }
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
                if model.status == .connected {
                    Button("Snippets…", systemImage: "chevron.left.forwardslash.chevron.right") { pickingSnippet = true }
                    Button("Load an environment…", systemImage: "list.bullet.rectangle") { pickingEnv = true }
                    Button("Files…", systemImage: "folder") { browsing = true }
                    Button("Place a config file…", systemImage: "doc.badge.gearshape") { placing = true }
                    Button("Install a key on this host…", systemImage: "key") { installing = true }
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
    /// The Snippets key: the session's snippets, to run.
    let snippets: () -> Void

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
                Button(action: snippets) {
                    Image(systemName: "chevron.left.forwardslash.chevron.right")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(Dusk.accent)
                        .frame(maxWidth: .infinity, minHeight: 40)
                }
                .background(Dusk.button, in: RoundedRectangle(cornerRadius: 9))
                .accessibilityLabel("Snippets")
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

/// A key from the vault, or a new one, put in the connected host's
/// `~/.ssh/authorized_keys`: from then on it signs in without a password.
struct InstallKeySheet: View {
    @ObservedObject var model: SessionModel
    @EnvironmentObject private var app: AppModel
    /// What happened, said; nil when cancelled.
    let done: (String?) -> Void
    @State private var keys: [KeyRow] = []
    @State private var name = "iPhone"
    @State private var busy = false
    @State private var problem: String?

    var body: some View {
        NavigationStack {
            Form {
                if !keys.isEmpty {
                    Section {
                        ForEach(keys, id: \.id) { k in
                            Button {
                                install(k)
                            } label: {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(k.name).foregroundStyle(Dusk.text)
                                    Text("\(k.kind) · \(k.fingerprint)")
                                        .font(.system(size: 12, design: .monospaced))
                                        .foregroundStyle(Dusk.muted)
                                        .lineLimit(1)
                                }
                            }
                            .disabled(busy)
                        }
                    } header: {
                        SectionHeading(text: "Keys in the vault")
                    }
                    .listRowBackground(Dusk.surface)
                }
                Section {
                    TextField("Name", text: $name)
                    Button("Make an Ed25519 key and install it") {
                        if let k = app.generateKey(name: name) {
                            keys = app.keys()
                            install(k)
                        }
                    }
                    .disabled(busy || name.trimmingCharacters(in: .whitespaces).isEmpty)
                } header: {
                    SectionHeading(text: "A new key")
                } footer: {
                    Text("Kept in the vault, encrypted; only its public line goes to \(model.spec).")
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)
                if let problem {
                    Text(problem).foregroundStyle(Dusk.badText).listRowBackground(Dusk.badBg)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.page.ignoresSafeArea())
            .navigationTitle("Install a key")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { done(nil) }
                }
                if busy {
                    ToolbarItem(placement: .confirmationAction) { ProgressView() }
                }
            }
            .onAppear { keys = app.keys() }
        }
        .preferredColorScheme(.dark)
    }

    private func install(_ key: KeyRow) {
        busy = true
        problem = nil
        Task {
            do {
                let added = try await model.install(key: key.public)
                done(added
                    ? "\(key.name) is on \(model.spec): it signs in from now on."
                    : "\(key.name) was on \(model.spec) already.")
            } catch {
                problem = "\(error)"
                busy = false
            }
        }
    }
}
