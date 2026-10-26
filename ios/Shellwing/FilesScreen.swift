import SwiftUI
import UniformTypeIdentifiers

/// A session's files over SFTP, as the desktop's Files panel has them: a
/// folder at a time, opened, made, renamed, removed; downloaded to the
/// phone (shared from there) and uploaded from it.
struct FilesScreen: View {
    @ObservedObject var model: SessionModel
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var dir = ""
    @State private var entries: [FileEntry] = []
    @State private var loading = false
    @State private var busy: String?
    @State private var problem: String?
    @State private var said: String?
    @State private var importing = false
    @State private var naming: Naming?
    @State private var removing: FileEntry?
    @State private var sharing: SharedFile?
    @State private var showHidden = false
    @State private var editing: RemoteText?

    /// A name asked for: a new folder, or an entry renamed.
    struct Naming: Identifiable {
        let id = UUID()
        let renaming: FileEntry?
        var text: String
    }

    var body: some View {
        NavigationStack {
            List {
                if let problem {
                    Text(problem).foregroundStyle(Dusk.badText).listRowBackground(Dusk.badBg)
                }
                if !loading && entries.isEmpty && problem == nil {
                    Text(tr("Nothing here.")).foregroundStyle(Dusk.muted).listRowBackground(Dusk.surface)
                }
                ForEach(shown, id: \.name) { e in
                    row(e)
                        .listRowBackground(Dusk.surface)
                        .swipeActions {
                            Button(tr("Delete"), role: .destructive) { removing = e }
                            Button(tr("Rename")) { naming = Naming(renaming: e, text: e.name) }.tint(Dusk.button)
                        }
                        .contextMenu { menu(e) }
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.panel.ignoresSafeArea())
            .refreshable { await list(dir) }
            .overlay {
                if loading && entries.isEmpty { ProgressView() }
            }
            .safeAreaInset(edge: .bottom) {
                if let text = busy ?? said {
                    HStack(spacing: 10) {
                        if busy != nil { ProgressView() }
                        Text(text).font(.system(size: 13)).foregroundStyle(Dusk.text)
                        Spacer()
                    }
                    .padding(12)
                    .background(Dusk.raised.opacity(0.97))
                }
            }
            .navigationTitle(name(of: dir))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(tr("Done")) { dismiss() } }
                ToolbarItem(placement: .principal) { pathMenu }
                ToolbarItem(placement: .primaryAction) {
                    Menu {
                        Button(tr("Upload from Files…"), systemImage: "square.and.arrow.up") { importing = true }
                        Button(tr("New folder"), systemImage: "folder.badge.plus") {
                            naming = Naming(renaming: nil, text: "")
                        }
                        Toggle(tr("Hidden files"), systemImage: "eye", isOn: $showHidden)
                        Button(tr("Refresh"), systemImage: "arrow.clockwise") { Task { await list(dir) } }
                    } label: { Image(systemName: "plus") }
                    .disabled(busy != nil || dir.isEmpty)
                    .accessibilityLabel(tr("Add"))
                }
            }
            .fileImporter(isPresented: $importing, allowedContentTypes: [.item, .folder], allowsMultipleSelection: true) { result in
                if case .success(let urls) = result { upload(urls) }
            }
            .alert(naming?.renaming == nil ? tr("New folder") : tr("Rename"), isPresented: Binding(
                get: { naming != nil },
                set: { if !$0 { naming = nil } }
            )) {
                TextField(tr("Name"), text: Binding(
                    get: { naming?.text ?? "" },
                    set: { naming?.text = $0 }
                ))
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                Button(tr("Cancel"), role: .cancel) { naming = nil }
                Button(naming?.renaming == nil ? tr("Make") : tr("Rename")) { named() }
            }
            .confirmationDialog(
                tr("Delete {name}?", ["name": removing?.name ?? ""]),
                isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
                titleVisibility: .visible
            ) {
                Button(tr("Delete"), role: .destructive) {
                    if let e = removing { remove(e) }
                }
            } message: {
                Text(removing?.dir == true ? tr("The folder and everything in it are deleted on {host}.", ["host": model.spec]) : tr("It is deleted on {host}.", ["host": model.spec]))
            }
            .sheet(item: $sharing) { f in
                ShareSheet(items: [f.url]).ignoresSafeArea()
            }
            .sheet(item: $editing, onDismiss: { Task { await list(dir) } }) { r in
                RemoteTextEditor(model: model, file: r)
            }
            .task {
                guard dir.isEmpty else { return }
                do {
                    let home = try await handle().filesHome()
                    await list(home)
                } catch {
                    problem = "\(error)"
                }
            }
        }
        .preferredColorScheme(.dark)
    }

    private var shown: [FileEntry] {
        showHidden ? entries : entries.filter { !$0.name.hasPrefix(".") }
    }

    /// The folder's path, its parents to go up to.
    private var pathMenu: some View {
        Menu {
            ForEach(parents(of: dir), id: \.self) { p in
                Button(p) { Task { await list(p) } }
            }
        } label: {
            HStack(spacing: 4) {
                Text(name(of: dir)).font(.system(size: 16, weight: .semibold)).lineLimit(1)
                Image(systemName: "chevron.down").font(.system(size: 11, weight: .semibold))
            }
            .foregroundStyle(Dusk.text)
        }
        .disabled(dir.isEmpty || dir == "/")
    }

    private func row(_ e: FileEntry) -> some View {
        Button {
            if e.dir {
                Task { await list(join(dir, e.name)) }
            } else if e.size <= RemoteText.most {
                edit(e)
            } else {
                download(e)
            }
        } label: {
            HStack(spacing: 12) {
                Image(systemName: e.dir ? "folder.fill" : (e.link ? "link" : "doc"))
                    .font(.system(size: 17))
                    .foregroundStyle(e.dir ? Dusk.accent : Dusk.soft)
                    .frame(width: 24)
                VStack(alignment: .leading, spacing: 2) {
                    Text(e.name).font(.system(size: 15)).foregroundStyle(Dusk.text).lineLimit(1)
                    Text(detail(e)).font(.system(size: 12)).foregroundStyle(Dusk.muted)
                }
                Spacer(minLength: 0)
                if e.dir {
                    Image(systemName: "chevron.right").font(.system(size: 12, weight: .semibold)).foregroundStyle(Dusk.muted)
                }
            }
        }
        .disabled(busy != nil)
    }

    @ViewBuilder
    private func menu(_ e: FileEntry) -> some View {
        if !e.dir && e.size <= RemoteText.most {
            Button(tr("Edit"), systemImage: "square.and.pencil") { edit(e) }
        }
        Button(e.dir ? tr("Download the folder") : tr("Download"), systemImage: "square.and.arrow.down") { download(e) }
        if !e.dir {
            Button(tr("Keep in the vault"), systemImage: "lock.doc") { keep(e) }
        }
        Button(tr("Rename"), systemImage: "pencil") { naming = Naming(renaming: e, text: e.name) }
        Button(tr("Copy the path"), systemImage: "doc.on.doc") { UIPasteboard.general.string = join(dir, e.name) }
        Button(tr("Delete"), systemImage: "trash", role: .destructive) { removing = e }
    }

    private func detail(_ e: FileEntry) -> String {
        var parts: [String] = []
        if !e.dir { parts.append(ByteCountFormatter.string(fromByteCount: Int64(e.size), countStyle: .file)) }
        if let m = e.modified {
            parts.append(Date(timeIntervalSince1970: TimeInterval(m)).formatted(date: .abbreviated, time: .shortened))
        }
        return parts.isEmpty ? (e.dir ? tr("Folder") : "") : parts.joined(separator: " · ")
    }

    // MARK: Doing

    private func handle() throws -> MobileSession {
        guard let h = model.handle, model.status == .connected else { throw MobileError.Failed(tr("Not connected.")) }
        return h
    }

    private func list(_ path: String) async {
        loading = true
        defer { loading = false }
        do {
            let got = try await handle().filesList(dir: path)
            entries = got
            dir = path
            problem = nil
        } catch {
            problem = "\(error)"
        }
    }

    private func named() {
        guard let n = naming else { return }
        naming = nil
        let text = n.text.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty, !text.contains("/") else {
            show(tr("A name without /, please."))
            return
        }
        run(n.renaming == nil ? tr("Making {name}…", ["name": text]) : tr("Renaming…")) {
            if let old = n.renaming {
                try await handle().filesRename(from: join(dir, old.name), to: join(dir, text))
            } else {
                try await handle().filesMkdir(path: join(dir, text))
            }
            return nil
        }
    }

    private func remove(_ e: FileEntry) {
        removing = nil
        run(tr("Deleting {name}…", ["name": e.name])) {
            try await handle().filesRemove(path: join(dir, e.name))
            return tr("{name} deleted.", ["name": "\(e.name)"])
        }
    }

    /// Downloads to a folder of the app's own, then offers it to share or
    /// save in Files.
    private func download(_ e: FileEntry) {
        let into = FileManager.default.temporaryDirectory
            .appendingPathComponent("downloads", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let local = into.appendingPathComponent(e.name)
        run(tr("Downloading {name}…", ["name": e.name]), reload: false) {
            try FileManager.default.createDirectory(at: into, withIntermediateDirectories: true)
            try await handle().filesDownload(remote: join(dir, e.name), local: local.path)
            sharing = SharedFile(url: local)
            return nil
        }
    }

    /// Opens a text file to edit; one that is not text is offered to share.
    private func edit(_ e: FileEntry) {
        let into = FileManager.default.temporaryDirectory
            .appendingPathComponent("edits", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let local = into.appendingPathComponent(e.name)
        let remote = join(dir, e.name)
        run(tr("Opening {name}…", ["name": e.name]), reload: false) {
            try FileManager.default.createDirectory(at: into, withIntermediateDirectories: true)
            try await handle().filesDownload(remote: remote, local: local.path)
            let data = try Data(contentsOf: local)
            if let text = String(data: data, encoding: .utf8), !data.contains(0) {
                editing = RemoteText(remote: remote, local: local, text: text, size: e.size, modified: e.modified)
            } else {
                sharing = SharedFile(url: local)
            }
            return nil
        }
    }

    private func upload(_ urls: [URL]) {
        let names = urls.map(\.lastPathComponent)
        run(urls.count == 1 ? tr("Uploading {name}…", ["name": names[0]]) : trN(urls.count, "Uploading {n} item…", "Uploading {n} items…")) {
            for url in urls {
                let scoped = url.startAccessingSecurityScopedResource()
                defer { if scoped { url.stopAccessingSecurityScopedResource() } }
                try await handle().filesUpload(local: url.path, remote: join(dir, url.lastPathComponent))
            }
            return urls.count == 1 ? tr("{name} uploaded.", ["name": names[0]]) : trN(urls.count, "{n} item uploaded.", "{n} items uploaded.")
        }
    }

    /// The file kept in the vault as a config file, to place on other hosts.
    private func keep(_ e: FileEntry) {
        guard let core = app.core, let h = model.handle else { return }
        run(tr("Keeping {name}…", ["name": e.name]), reload: false) {
            _ = try await core.keepHostFile(session: h, path: join(dir, e.name))
            return tr("{name} is in the vault: Library › Files.", ["name": "\(e.name)"])
        }
    }

    /// Does `work` with `doing` said; then says what it returns, or why it failed.
    private func run(_ doing: String, reload: Bool = true, _ work: @escaping () async throws -> String?) {
        busy = doing
        Task {
            do {
                let done = try await work()
                busy = nil
                if let done { show(done) }
            } catch {
                busy = nil
                show("\(error)")
            }
            if reload { await list(dir) }
        }
    }

    private func show(_ text: String) {
        said = text
        Task {
            try? await Task.sleep(for: .seconds(4))
            if said == text { said = nil }
        }
    }
}

struct SharedFile: Identifiable {
    let id = UUID()
    let url: URL
}

/// The system's share sheet: Save to Files, AirDrop, open in an app.
struct ShareSheet: UIViewControllerRepresentable {
    let items: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: items, applicationActivities: nil)
    }

    func updateUIViewController(_ vc: UIActivityViewController, context: Context) {}
}

/// `name` in the folder `dir`.
func join(_ dir: String, _ name: String) -> String {
    dir.hasSuffix("/") ? dir + name : dir + "/" + name
}

/// A path's last part; `/` for the root.
private func name(of path: String) -> String {
    if path.isEmpty { return tr("Files") }
    let last = path.split(separator: "/").last.map(String.init)
    return last ?? "/"
}

/// The folders `path` is in, nearest first, up to `/`.
private func parents(of path: String) -> [String] {
    var out: [String] = []
    var parts = path.split(separator: "/").map(String.init)
    while !parts.isEmpty {
        parts.removeLast()
        out.append("/" + parts.joined(separator: "/"))
    }
    return out
}

/// A remote text file being edited: where it is, its copy here, and what
/// the server had when it was opened.
struct RemoteText: Identifiable {
    /// Larger than this is not a file to edit on a phone.
    static let most: UInt64 = 1024 * 1024

    let id = UUID()
    let remote: String
    let local: URL
    let text: String
    let size: UInt64
    let modified: UInt64?
}

/// A remote file's text, saved back over SFTP; if the server's changed
/// since it was opened, asked before it is replaced.
struct RemoteTextEditor: View {
    @ObservedObject var model: SessionModel
    let file: RemoteText
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var saving = false
    @State private var problem: String?
    @State private var changedThere = false
    @State private var asked = false

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                if let problem {
                    Text(problem)
                        .font(.system(size: 13))
                        .foregroundStyle(Dusk.badText)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(10)
                        .background(Dusk.badBg)
                }
                TextEditor(text: $text)
                    .font(.system(size: 13, design: .monospaced))
                    .scrollContentBackground(.hidden)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .padding(.horizontal, 8)
            }
            .foregroundStyle(Dusk.text)
            .background(Dusk.field.ignoresSafeArea())
            .navigationTitle(file.remote.split(separator: "/").last.map(String.init) ?? file.remote)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(tr("Cancel")) { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    if saving { ProgressView() } else {
                        Button(tr("Save")) { save(force: false) }.disabled(text == file.text)
                    }
                }
            }
            .confirmationDialog(
                tr("{path} changed on {host} since it was opened.", ["path": file.remote, "host": model.spec]),
                isPresented: $changedThere,
                titleVisibility: .visible
            ) {
                Button(tr("Replace it with this"), role: .destructive) { save(force: true) }
            }
            .onAppear {
                if !asked { text = file.text; asked = true }
            }
        }
        .preferredColorScheme(.dark)
    }

    private func save(force: Bool) {
        guard let h = model.handle else { return }
        saving = true
        Task {
            defer { saving = false }
            do {
                if !force {
                    // What the server has now, against what it had.
                    let dir = String(file.remote[..<(file.remote.lastIndex(of: "/") ?? file.remote.startIndex)])
                    let name = String(file.remote.split(separator: "/").last ?? "")
                    let now = try await h.filesList(dir: dir.isEmpty ? "/" : dir).first { $0.name == name }
                    if let now, now.size != file.size || now.modified != file.modified {
                        changedThere = true
                        return
                    }
                }
                try text.write(to: file.local, atomically: true, encoding: .utf8)
                try await h.filesUpload(local: file.local.path, remote: file.remote)
                dismiss()
            } catch {
                problem = tr("Not saved: {error}", ["error": "\(error)"])
            }
        }
    }
}
