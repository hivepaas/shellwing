import SwiftUI
import UniformTypeIdentifiers

// MARK: In the Library

/// Config files kept in the vault, by category: a kubeconfig, a .npmrc,
/// placed on a host from a session.
struct ConfigFilesList: View {
    @EnvironmentObject private var app: AppModel
    @State private var files: [ConfigFileRow] = []
    @State private var editing: ConfigFileRow?

    var body: some View {
        List {
            if files.isEmpty {
                Text(tr("No config files yet: files kept in the vault (a kubeconfig, an .npmrc), placed on a host from a session."))
                    .foregroundStyle(Dusk.muted)
                    .listRowBackground(Dusk.surface)
            }
            ForEach(categories, id: \.self) { cat in
                Section {
                    ForEach(files.filter { $0.category == cat }, id: \.id) { f in
                        Button { editing = f } label: { ConfigFileLabel(file: f) }
                            .listRowBackground(Dusk.surface)
                            .swipeActions {
                                Button(tr("Delete"), role: .destructive) {
                                    app.forget(f.id)
                                    reload()
                                }
                            }
                    }
                } header: {
                    SectionHeading(text: cat.isEmpty ? tr("No category") : cat)
                }
            }
        }
        .scrollContentBackground(.hidden)
        .toolbar {
            Button {
                editing = ConfigFileRow(id: "", name: "", category: "", path: "~/", mode: 0o600, size: 0, binary: false, stays: false, scope: "")
            } label: { Image(systemName: "plus") }
            .accessibilityLabel(tr("New config file"))
        }
        .sheet(item: $editing, onDismiss: reload) { f in
            ConfigFileEditor(file: f)
        }
        .onAppear(perform: reload)
    }

    private var categories: [String] {
        var seen: [String] = []
        for f in files where !seen.contains(f.category) { seen.append(f.category) }
        return seen
    }

    private func reload() {
        files = app.configFiles(for: nil)
    }
}

extension ConfigFileRow: Identifiable {}

struct ConfigFileLabel: View {
    let file: ConfigFileRow

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Text(file.name).font(.system(size: 15, weight: .medium)).foregroundStyle(Dusk.text)
                if file.stays {
                    Text(tr("stays")).font(.system(size: 11)).foregroundStyle(Dusk.muted)
                        .padding(.horizontal, 6).padding(.vertical, 1)
                        .background(Dusk.chip, in: Capsule())
                }
            }
            Text(file.path)
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(Dusk.soft)
                .lineLimit(1)
        }
    }
}

/// A config file: where it goes, its permissions, where it is offered, and
/// its text (a binary one is kept as it is, or replaced from Files).
struct ConfigFileEditor: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    @State var file: ConfigFileRow
    @State private var text = ""
    /// The text was changed, or a file imported: saved with it.
    @State private var changed = false
    @State private var mode = "600"
    @State private var importing = false
    @State private var problem: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField(tr("Name"), text: $file.name)
                    TextField(tr("Category"), text: $file.category)
                }
                .listRowBackground(Dusk.surface)
                Section {
                    TextField("~/.kube/config", text: $file.path)
                        .font(.system(.body, design: .monospaced))
                    HStack {
                        Text(tr("Permissions"))
                        Spacer()
                        TextField("600", text: $mode)
                            .font(.system(.body, design: .monospaced))
                            .multilineTextAlignment(.trailing)
                            .keyboardType(.numberPad)
                            .frame(width: 80)
                    }
                    Toggle(tr("Placed to stay"), isOn: $file.stays)
                } header: {
                    SectionHeading(text: tr("On the host"))
                } footer: {
                    Text(file.stays
                        ? tr("Placed, it stays when the session ends.")
                        : tr("Placed, it is taken away when the session ends."))
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)
                Section {
                    ScopePicker(scope: $file.scope)
                }
                .listRowBackground(Dusk.surface)
                Section {
                    if file.binary && !changed {
                        Text(tr("Binary, {size}: kept as it is.", ["size": ByteCountFormatter.string(fromByteCount: Int64(file.size), countStyle: .file)]))
                            .foregroundStyle(Dusk.muted)
                    } else {
                        TextEditor(text: Binding(get: { text }, set: { text = $0; changed = true }))
                            .font(.system(size: 13, design: .monospaced))
                            .frame(minHeight: 200)
                    }
                    Button(tr("Replace from Files…"), systemImage: "doc.badge.arrow.up") { importing = true }
                } header: {
                    SectionHeading(text: tr("Content"))
                }
                .listRowBackground(Dusk.surface)
                if let problem {
                    Text(problem).foregroundStyle(Dusk.badText).listRowBackground(Dusk.badBg)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.page.ignoresSafeArea())
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .navigationTitle(file.id.isEmpty ? tr("New config file") : file.name)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(tr("Cancel")) { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button(tr("Save"), action: save).disabled(file.name.isEmpty || file.path.isEmpty)
                }
            }
            .fileImporter(isPresented: $importing, allowedContentTypes: [.item]) { result in
                guard case .success(let url) = result else { return }
                let scoped = url.startAccessingSecurityScopedResource()
                defer { if scoped { url.stopAccessingSecurityScopedResource() } }
                guard let data = try? Data(contentsOf: url), let s = String(data: data, encoding: .utf8) else {
                    problem = tr("Only a text file can be imported here.")
                    return
                }
                text = s
                changed = true
                if file.name.isEmpty { file.name = url.lastPathComponent }
            }
            .onAppear {
                mode = String(file.mode, radix: 8)
                if !file.id.isEmpty, !file.binary { text = app.configFileText(file.id) ?? "" }
            }
        }
        .preferredColorScheme(.dark)
    }

    private func save() {
        guard let m = UInt32(mode, radix: 8), m <= 0o777 else {
            problem = tr("Permissions: three digits from 0 to 7, such as 600.")
            return
        }
        file.mode = m
        // A new one is saved with its text, even empty.
        let content = changed || file.id.isEmpty ? text : nil
        if let why = app.save(configFile: file, content: content) {
            problem = why
        } else {
            dismiss()
        }
    }
}

// MARK: In a session

/// The config files offered on the session's host; one tapped is looked
/// for there, and placed the way chosen, as the desktop asks.
struct PlaceConfigSheet: View {
    @ObservedObject var model: SessionModel
    @EnvironmentObject private var app: AppModel
    /// What happened, said; nil when nothing was.
    let done: (String?) -> Void
    @State private var files: [ConfigFileRow] = []
    @State private var asking: Asking?
    @State private var busy = false
    @State private var problem: String?

    /// What is at the file's path, and so the ways to place it.
    struct Asking: Identifiable {
        let id = UUID()
        let file: ConfigFileRow
        let looked: Looked
    }

    var body: some View {
        NavigationStack {
            List {
                if files.isEmpty {
                    Text(tr("No config files for this host. Keep them in Library › Files."))
                        .foregroundStyle(Dusk.muted)
                        .listRowBackground(Dusk.surface)
                }
                ForEach(files, id: \.id) { f in
                    Button { look(f) } label: { ConfigFileLabel(file: f) }
                        .disabled(busy)
                        .listRowBackground(Dusk.surface)
                }
                if let problem {
                    Text(problem).foregroundStyle(Dusk.badText).listRowBackground(Dusk.badBg)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.panel.ignoresSafeArea())
            .navigationTitle(tr("Place a config file"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(tr("Cancel")) { done(nil) } }
                if busy { ToolbarItem(placement: .confirmationAction) { ProgressView() } }
            }
            .confirmationDialog(
                title,
                isPresented: Binding(get: { asking != nil }, set: { if !$0 { asking = nil } }),
                titleVisibility: .visible,
                presenting: asking
            ) { a in
                ForEach(choices(a), id: \.label) { c in
                    Button(c.label) { place(a, c.how) }
                }
                Button(tr("Cancel"), role: .cancel) {}
            }
            .onAppear { files = app.configFiles(for: model.spec) }
        }
        .preferredColorScheme(.dark)
    }

    private var title: String {
        guard let a = asking else { return "" }
        return a.looked.found == .other && !a.looked.ours
            ? tr("{path} is there already…", ["path": a.looked.full])
            : tr("Place at {path}…", ["path": a.looked.full])
    }

    /// A way to place it; `how` nil keeps the one placed already.
    struct Choice {
        let label: String
        let how: PlaceHow?
    }

    /// The ways, the file's own first (to stay, or until the session ends).
    private func choices(_ a: Asking) -> [Choice] {
        let stays = a.file.stays
        func first(_ stay: [Choice], _ until: [Choice]) -> [Choice] {
            stays ? stay + until : until + stay
        }
        if a.looked.ours {
            let again = a.looked.found == .same ? [] : [Choice(label: tr("Replace the one placed until the session ends"), how: .until)]
            return first([Choice(label: tr("Keep the one placed here"), how: nil)], again)
        }
        switch a.looked.found {
        case .nothing, .same:
            return first(
                [Choice(label: tr("Place it to stay"), how: .stay)],
                [Choice(label: tr("Place it until the session ends"), how: .until)]
            )
        case .other:
            return first(
                [
                    Choice(label: tr("Replace it, keeping the old one"), how: .backup),
                    Choice(label: tr("Replace it"), how: .over),
                ],
                [Choice(label: tr("Replace it until the session ends"), how: .untilOver)]
            )
        }
    }

    private func look(_ f: ConfigFileRow) {
        guard let core = app.core, let h = model.handle, model.status == .connected else {
            problem = tr("Not connected.")
            return
        }
        busy = true
        problem = nil
        Task {
            defer { busy = false }
            do {
                let looked = try await core.lookToPlace(session: h, id: f.id, path: f.path)
                if looked.found == .same && !looked.ours {
                    done(tr("{path} holds it already.", ["path": looked.full]))
                } else {
                    asking = Asking(file: f, looked: looked)
                }
            } catch {
                problem = tr("Cannot place {name}: {error}", ["name": "\(f.name)", "error": "\(error)"])
            }
        }
    }

    private func place(_ a: Asking, _ how: PlaceHow?) {
        guard let core = app.core, let h = model.handle else { return }
        busy = true
        Task {
            defer { busy = false }
            do {
                if let how {
                    try await core.place(session: h, id: a.file.id, full: a.looked.full, how: how)
                } else {
                    try await core.keepPlaced(session: h, path: a.looked.full)
                }
                model.refreshPlaced()
                let until = how == .until || how == .untilOver
                done(until
                    ? tr("{name} is at {path} until the session ends.", ["name": "\(a.file.name)", "path": a.looked.full])
                    : tr("{name} is at {path}.", ["name": "\(a.file.name)", "path": a.looked.full]))
            } catch {
                problem = tr("Cannot place {name}: {error}", ["name": "\(a.file.name)", "error": "\(error)"])
            }
        }
    }
}
