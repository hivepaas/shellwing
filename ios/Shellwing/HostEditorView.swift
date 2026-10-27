import SwiftUI

/// A host as the desktop's editor has it: how it is reached and signed in
/// to, the hosts it goes through, ports forwarded, and how it looks.
struct HostEditorView: View {
    @EnvironmentObject private var app: AppModel
    /// Called with the saved host's id, to connect to; nil when cancelled.
    let done: (String?) -> Void

    @State private var edit: HostEdit
    @State private var port: String
    @State private var tags: String
    @State private var keepalive: String
    @State private var forward = ""
    @State private var forwardSaid: String?
    @State private var keys: [KeyRow] = []
    /// A key made here: the host does not have it yet.
    @State private var madeKey: KeyRow?
    @State private var problem: String?
    @State private var confirmDelete = false

    init(edit: HostEdit, done: @escaping (String?) -> Void) {
        self.done = done
        _edit = State(initialValue: edit)
        _port = State(initialValue: edit.port.map(String.init) ?? "")
        _tags = State(initialValue: edit.tags.joined(separator: ", "))
        _keepalive = State(initialValue: edit.keepalive.map(String.init) ?? "")
    }

    /// The shell kept on the host: tmux's session `shellwing`, made or
    /// joined after each connect (the host's run command, so the desktop
    /// joins it too); nothing where tmux is not.
    static let tmux = "command -v tmux >/dev/null && tmux new -A -s shellwing"

    /// A blank host, for +.
    static func blank() -> HostEdit {
        HostEdit(
            id: nil, name: "", alias: nil, address: "", port: nil, user: nil, group: nil,
            tags: [], jump: [], forward: [], auth: .auto, key: nil, theme: nil, color: nil,
            favorite: false, localOnly: false, forwardAgent: false, keepalive: nil, hostKey: nil, run: nil
        )
    }

    var body: some View {
        NavigationStack {
            Form {
                connection
                signIn
                route
                forwarding
                organize
                terminal
                advanced
                if let problem {
                    Text(problem).foregroundStyle(Dusk.badText).listRowBackground(Dusk.badBg)
                }
                Section {
                    Button(tr("Save and connect")) { save(connect: true) }
                        .font(.system(size: 17, weight: .semibold))
                        .frame(maxWidth: .infinity, minHeight: 44)
                        .listRowBackground(Dusk.accent)
                        .foregroundStyle(Dusk.onAccent)
                        .disabled(blankAddress)
                }
                if edit.id != nil {
                    Section {
                        Button(tr("Delete…"), role: .destructive) { confirmDelete = true }
                            .frame(maxWidth: .infinity)
                            .confirmationDialog(
                                tr("Delete {title} from hosts.toml?", ["title": title]),
                                isPresented: $confirmDelete,
                                titleVisibility: .visible
                            ) {
                                Button(tr("Delete"), role: .destructive) {
                                    if let id = edit.id { app.delete(hostId: id) }
                                    done(nil)
                                }
                            }
                    }
                    .listRowBackground(Dusk.surface)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.page.ignoresSafeArea())
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .navigationTitle(edit.id == nil ? tr("New host") : title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(tr("Cancel")) { done(nil) }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(tr("Save")) { save(connect: false) }.disabled(blankAddress)
                }
            }
            .onAppear { keys = app.keys() }
        }
        .preferredColorScheme(.dark)
    }

    private var title: String { edit.name.isEmpty ? edit.address : edit.name }
    private var blankAddress: Bool { edit.address.trimmingCharacters(in: .whitespaces).isEmpty }

    // MARK: Sections

    private var connection: some View {
        Section {
            LabeledContent(tr("Name")) {
                TextField(edit.address.isEmpty ? "web-1" : edit.address, text: $edit.name)
            }
            LabeledContent(tr("Address")) {
                TextField("host.example.com", text: $edit.address)
                    .keyboardType(.URL)
                    .font(.system(.body, design: .monospaced))
            }
            LabeledContent(tr("User")) {
                TextField(tr("as the server says"), text: text($edit.user))
                    .font(.system(.body, design: .monospaced))
            }
            LabeledContent(tr("Port")) {
                TextField("22", text: $port)
                    .keyboardType(.numberPad)
                    .font(.system(.body, design: .monospaced))
            }
            LabeledContent(tr("Alias")) {
                TextField(tr("optional"), text: text($edit.alias))
                    .font(.system(.body, design: .monospaced))
            }
        } header: {
            SectionHeading(text: tr("Connection"))
        }
        .multilineTextAlignment(.trailing)
        .listRowBackground(Dusk.surface)
    }

    private var signIn: some View {
        Section {
            Picker(tr("Sign in"), selection: $edit.auth) {
                Text(tr("Automatic")).tag(AuthWay.auto)
                Text(tr("Key")).tag(AuthWay.key)
                Text(tr("Password")).tag(AuthWay.password)
                if edit.auth == .agent { Text(tr("Agent")).tag(AuthWay.agent) }
            }
            .pickerStyle(.segmented)
            if edit.auth == .key {
                Picker(tr("Key"), selection: text($edit.key)) {
                    Text(tr("Choose a key")).tag("")
                    ForEach(keys, id: \.id) { k in
                        Text("\(k.name) · \(k.kind)").tag("vault:\(k.id)")
                    }
                    // A key file written on a desktop: kept as it is.
                    if let k = edit.key, !k.isEmpty, !k.hasPrefix("vault:") {
                        Text(k).tag(k)
                    }
                }
                Button(title.isEmpty ? tr("Make a key for this host") : tr("Make a key for {host}", ["host": title]), systemImage: "plus") {
                    makeKey()
                }
                if let k = madeKey, edit.key == "vault:\(k.id)" {
                    Text(tr("{name} is new: the host does not have it yet. Connect once another way (a password), then install it from the session's menu.", ["name": "\(k.name)"]))
                        .font(.system(size: 13))
                        .foregroundStyle(Dusk.amber)
                }
            }
        } header: {
            SectionHeading(text: tr("Sign in"))
        } footer: {
            Text(signInHint).foregroundStyle(Dusk.muted)
        }
        .listRowBackground(Dusk.surface)
    }

    private var signInHint: String {
        switch edit.auth {
        case .auto: return tr("Tries the keys in the vault, then asks for a password, like ssh.")
        case .key:
            return edit.key?.hasPrefix("vault:") == true
                ? tr("Kept in the vault: no passphrase to ask for.")
                : tr("A key file of another device: this phone signs in with the vault's keys.")
        case .password: return tr("Asked for when connecting, and can be saved in the vault.")
        case .agent: return tr("Uses the keys in an ssh-agent: a desktop's; a phone has none.")
        }
    }

    /// The jump hosts, outermost first, then this host.
    private var route: some View {
        Section {
            ForEach(Array(edit.jump.enumerated()), id: \.offset) { i, j in
                HStack {
                    Text("\(i + 1)").font(.system(size: 13, design: .monospaced)).foregroundStyle(Dusk.muted)
                    Text(j).font(.system(.body, design: .monospaced))
                }
            }
            .onDelete { edit.jump.remove(atOffsets: $0) }
            .onMove { edit.jump.move(fromOffsets: $0, toOffset: $1) }
            let choices = app.core?.jumpChoices(except: edit.name.isEmpty ? nil : edit.name)
                .filter { !edit.jump.contains($0) } ?? []
            if !choices.isEmpty {
                Menu {
                    ForEach(choices, id: \.self) { c in
                        Button(c) { edit.jump.append(c) }
                    }
                } label: {
                    Label(tr("Add a jump"), systemImage: "plus")
                }
            }
        } header: {
            SectionHeading(text: tr("Route"))
        } footer: {
            Text(edit.jump.isEmpty
                ? tr("Straight to the host. A jump goes through another host first (a bastion).")
                : (edit.jump + [title]).joined(separator: " → "))
                .foregroundStyle(Dusk.muted)
        }
        .listRowBackground(Dusk.surface)
    }

    private var forwarding: some View {
        Section {
            ForEach(edit.forward, id: \.self) { f in
                Text(f).font(.system(.body, design: .monospaced))
            }
            .onDelete { edit.forward.remove(atOffsets: $0) }
            HStack {
                TextField("-L 8080:localhost:80", text: $forward)
                    .font(.system(.body, design: .monospaced))
                    .onSubmit(addForward)
                Button(tr("Add"), action: addForward).disabled(forward.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            if let forwardSaid {
                Text(forwardSaid).font(.system(size: 13)).foregroundStyle(Dusk.badText)
            }
        } header: {
            SectionHeading(text: tr("Port forwarding"))
        } footer: {
            Text("-L [bind:]port:host:hostport · -R [bind:]port:host:hostport · -D [bind:]port\n" + tr("Set up every time this host connects."))
                .foregroundStyle(Dusk.muted)
        }
        .listRowBackground(Dusk.surface)
    }

    private var organize: some View {
        Section {
            LabeledContent(tr("Group")) {
                TextField(tr("None"), text: text($edit.group)).multilineTextAlignment(.trailing)
            }
            let groups = app.groups()
            if !groups.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 6) {
                        ForEach(groups, id: \.self) { g in
                            let on = edit.group == g
                            Button(g) { edit.group = on ? nil : g }
                                .font(.system(size: 13, weight: on ? .semibold : .regular))
                                .padding(.horizontal, 10)
                                .frame(height: 30)
                                .background(on ? Dusk.selected : Dusk.button, in: Capsule())
                                .overlay(Capsule().stroke(on ? Dusk.accent : .clear))
                                .foregroundStyle(Dusk.text)
                        }
                    }
                }
            }
            LabeledContent(tr("Tags")) {
                TextField("web, docker", text: $tags).multilineTextAlignment(.trailing)
            }
            HStack(spacing: 10) {
                Text(tr("Color"))
                Spacer()
                swatch(nil, color: Dusk.button)
                ForEach(hostColors(), id: \.name) { c in
                    swatch(c.name, color: Color(hex: c.rgb))
                }
            }
            Toggle(tr("Favorite: listed first"), isOn: $edit.favorite)
        } header: {
            SectionHeading(text: tr("Organize"))
        }
        .listRowBackground(Dusk.surface)
    }

    private func swatch(_ name: String?, color: Color) -> some View {
        let on = edit.color == name
        return Button { edit.color = name } label: {
            Circle().fill(color)
                .frame(width: 24, height: 24)
                .overlay(Circle().stroke(on ? Dusk.text : .clear, lineWidth: 2).padding(-3))
                .overlay {
                    if name == nil { Image(systemName: "slash.circle").font(.system(size: 13)).foregroundStyle(Dusk.muted) }
                }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(name ?? tr("No color"))
        .accessibilityAddTraits(on ? .isSelected : [])
    }

    private var terminal: some View {
        Section {
            NavigationLink {
                ThemePicker(selected: edit.theme, appDefault: true) { edit.theme = $0 }
            } label: {
                LabeledContent(tr("Theme")) {
                    Text(themeName).foregroundStyle(Dusk.muted)
                }
            }
            Toggle(tr("Keep the shell on the host (tmux)"), isOn: Binding(
                get: { edit.run == Self.tmux },
                set: { edit.run = $0 ? Self.tmux : nil }
            ))
            VStack(alignment: .leading, spacing: 6) {
                Text(tr("Run after connecting"))
                TextField("cd ~/apps && ls", text: text($edit.run))
                    .font(.system(.body, design: .monospaced))
            }
        } header: {
            SectionHeading(text: tr("Terminal"))
        } footer: {
            Text(tr("In tmux, a connection made again finds the same shell, its programs still running: the phone may lose it in the background. tmux has to be on the host; without it, the shell is as usual."))
                .foregroundStyle(Dusk.muted)
        }
        .listRowBackground(Dusk.surface)
    }

    private var themeName: String {
        guard let id = edit.theme else { return tr("Same as the app") }
        return app.core?.themes().first { $0.id == id }?.name ?? id
    }

    private var advanced: some View {
        Section {
            Picker(tr("Host keys"), selection: $edit.hostKey) {
                Text(tr("As in Settings")).tag(HostKeyWay?.none)
                Text(tr("Ask")).tag(HostKeyWay?.some(.ask))
                Text(tr("Accept new ones")).tag(HostKeyWay?.some(.acceptNew))
                Text(tr("Known ones only")).tag(HostKeyWay?.some(.strict))
            }
            LabeledContent(tr("Keepalive (s)")) {
                TextField(tr("as in Settings"), text: $keepalive)
                    .keyboardType(.numberPad)
                    .multilineTextAlignment(.trailing)
            }
            Toggle(tr("Keep on this device only"), isOn: $edit.localOnly)
        } header: {
            SectionHeading(text: tr("Advanced"))
        } footer: {
            Text(edit.localOnly
                ? tr("Not synced: other devices do not see this host.")
                : tr("Synced with your other devices when sync is on."))
                .foregroundStyle(Dusk.muted)
        }
        .listRowBackground(Dusk.surface)
    }

    // MARK: Doing

    /// An optional string's field: empty is nil.
    private func text(_ b: Binding<String?>) -> Binding<String> {
        Binding(get: { b.wrappedValue ?? "" }, set: { b.wrappedValue = $0.isEmpty ? nil : $0 })
    }

    private func addForward() {
        let f = forward.trimmingCharacters(in: .whitespaces)
        guard !f.isEmpty else { return }
        if let why = forwardProblem(spec: f) {
            forwardSaid = why
            return
        }
        forwardSaid = nil
        if !edit.forward.contains(f) { edit.forward.append(f) }
        forward = ""
    }

    private func makeKey() {
        let name = title.isEmpty ? "iPhone" : title
        guard let k = app.generateKey(name: name) else {
            problem = app.error
            app.error = nil
            return
        }
        keys = app.keys()
        madeKey = k
        edit.key = "vault:\(k.id)"
    }

    private func save(connect: Bool) {
        let p = port.trimmingCharacters(in: .whitespaces)
        if !p.isEmpty, UInt16(p) == nil {
            problem = tr("Port: 1 to 65535.")
            return
        }
        let k = keepalive.trimmingCharacters(in: .whitespaces)
        if !k.isEmpty, UInt64(k) == nil {
            problem = tr("Keepalive: seconds, 0 to 3600 (0: none).")
            return
        }
        var e = edit
        e.port = UInt16(p)
        e.keepalive = UInt64(k)
        e.tags = tags.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        do {
            let saved = try app.trySave(e)
            done(connect ? saved : nil)
        } catch {
            problem = "\(error)"
        }
    }
}
