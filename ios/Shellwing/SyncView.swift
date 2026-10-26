import SwiftUI
import UniformTypeIdentifiers

/// A folder the user gave the app to sync in (one in iCloud Drive, chosen in
/// Files): kept as a security-scoped bookmark, opened again at each start.
enum SyncFolder {
    private static let key = "sync.folder.bookmark"

    /// Keeps `url` (from the folder picker) for the next starts; its path.
    static func keep(_ url: URL) throws -> String {
        guard url.startAccessingSecurityScopedResource() else {
            throw MobileError.Failed(tr("shellwing may not use that folder."))
        }
        let data = try url.bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil)
        UserDefaults.standard.set(data, forKey: key)
        return url.path
    }

    /// Opens the kept folder to the app again, if there is one.
    static func resume() {
        guard let data = UserDefaults.standard.data(forKey: key) else { return }
        var stale = false
        guard let url = try? URL(resolvingBookmarkData: data, bookmarkDataIsStale: &stale) else { return }
        _ = url.startAccessingSecurityScopedResource()
        if stale, let fresh = try? url.bookmarkData() {
            UserDefaults.standard.set(fresh, forKey: key)
        }
    }

    static func forget() {
        UserDefaults.standard.removeObject(forKey: key)
    }
}

/// Settings › Sync: where the vault syncs, and joining one another device
/// set up, as the desktop's Settings › Sync.
struct SyncView: View {
    @EnvironmentObject private var app: AppModel

    var body: some View {
        Group {
            if let status = app.sync, status.place != nil {
                SyncingView(status: status)
            } else if case .some(let p) = app.sync?.pairing {
                PairingView(pairing: p)
            } else {
                SetUpSyncView()
            }
        }
        .navigationTitle(tr("Sync"))
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// Where it syncs, how it went, and the devices asking to join.
private struct SyncingView: View {
    @EnvironmentObject private var app: AppModel
    let status: SyncStatus
    @State private var stopping = false

    var body: some View {
        Form {
            Section {
                LabeledContent(tr("Place")) { Text(status.said ?? "").foregroundStyle(Dusk.soft) }
                LabeledContent(tr("Last synced")) {
                    if status.syncing {
                        ProgressView()
                    } else if let last = status.last {
                        Text(Date(timeIntervalSince1970: TimeInterval(last)), style: .relative)
                            .foregroundStyle(Dusk.soft)
                    } else {
                        Text(tr("not yet")).foregroundStyle(Dusk.muted)
                    }
                }
                if let error = status.error {
                    Text(error).foregroundStyle(Dusk.badText).font(.system(size: 14))
                }
                Button(tr("Sync now")) { app.core?.syncNow() }
                    .disabled(status.syncing)
            } footer: {
                Text(tr("End-to-end encrypted: the place holds only what the vault's key opens. It syncs every minute while shellwing is open, and when something changes."))
                    .foregroundStyle(Dusk.muted)
            }
            .listRowBackground(Dusk.surface)

            if !status.requests.isEmpty {
                Section {
                    ForEach(status.requests, id: \.id) { r in
                        VStack(alignment: .leading, spacing: 8) {
                            Text(tr("{device} asks to join", ["device": "\(r.device)"])).font(.system(size: 15, weight: .semibold))
                            Text(tr("Approve only if it shows {code}. It gets everything the vault holds: passwords, keys, snippets and hosts.", ["code": "\(r.code)"]))
                                .font(.system(size: 13))
                                .foregroundStyle(Dusk.label)
                            HStack {
                                Button(tr("Refuse"), role: .destructive) {
                                    app.core?.refuse(id: r.id)
                                    app.refreshSync()
                                }
                                Spacer()
                                Button(tr("Approve")) {
                                    app.core?.approve(id: r.id)
                                    app.refreshSync()
                                }
                                .buttonStyle(.borderedProminent)
                            }
                        }
                        .padding(.vertical, 4)
                    }
                } header: {
                    SectionHeading(text: tr("Devices asking"))
                }
                .listRowBackground(Dusk.surface)
            }

            Section {
                Button(tr("Stop syncing"), role: .destructive) { stopping = true }
                .confirmationDialog(tr("Stop syncing on this phone?"), isPresented: $stopping, titleVisibility: .visible) {
                    Button(tr("Stop syncing"), role: .destructive) {
                        do {
                            try app.core?.stopSync()
                            SyncFolder.forget()
                        } catch {
                            app.error = "\(error)"
                        }
                        app.refreshSync()
                    }
                } message: {
                    Text(tr("The vault stays here, and the copy at the place stays for your other devices."))
                }
            }
            .listRowBackground(Dusk.surface)
        }
        .scrollContentBackground(.hidden)
        .background(Dusk.panel.ignoresSafeArea())
    }
}

/// This phone's request to join, while another device answers.
private struct PairingView: View {
    @EnvironmentObject private var app: AppModel
    let pairing: Pairing

    var body: some View {
        VStack(spacing: 18) {
            switch pairing {
            case .waiting:
                ProgressView()
                Text(tr("Waiting for a device that syncs there…")).font(.headline)
                Text(tr("Open shellwing on it: it asks whether to let this phone in."))
                    .foregroundStyle(Dusk.label)
            case .compare(let device, let code):
                Text(tr("{device} answered", ["device": "\(device)"])).font(.headline)
                Text(code)
                    .font(.system(size: 34, weight: .semibold, design: .monospaced))
                    .padding(.horizontal, 18)
                    .padding(.vertical, 10)
                    .background(Dusk.field, in: RoundedRectangle(cornerRadius: 12))
                Text(tr("Approve on {device} if it shows the same code.", ["device": "\(device)"]))
                    .foregroundStyle(Dusk.label)
            case .joined:
                Image(systemName: "checkmark.circle.fill").font(.system(size: 44)).foregroundStyle(Dusk.ok)
                Text(tr("Joined: this phone syncs there now.")).font(.headline)
            case .refused:
                Text(tr("The other device refused to let this one in.")).foregroundStyle(Dusk.badText)
            case .expired:
                Text(tr("No device let this one in within 10 minutes. Ask again, or use the recovery key."))
                    .foregroundStyle(Dusk.badText)
            }
            Button(pairing == .waiting || isCompare ? tr("Cancel") : tr("Done")) {
                app.core?.cancelJoin()
                app.refreshSync()
            }
            .buttonStyle(.bordered)
            Spacer()
        }
        .multilineTextAlignment(.center)
        .padding(24)
        .foregroundStyle(Dusk.text)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Dusk.page.ignoresSafeArea())
    }

    private var isCompare: Bool {
        if case .compare = pairing { return true }
        return false
    }
}

/// Choosing a place, connecting, and joining what is there.
private struct SetUpSyncView: View {
    @EnvironmentObject private var app: AppModel

    enum Kind: String, CaseIterable, Identifiable {
        case webdav = "WebDAV", s3 = "S3", git = "Git", folder = "Folder"
        var id: String { rawValue }
    }

    // `-syncKind Git` at launch (debug builds) starts on that kind.
    @State private var kind: Kind = Kind(rawValue: UserDefaults.standard.string(forKey: "syncKind") ?? "") ?? .webdav
    @State private var url = ""
    @State private var user = ""
    @State private var password = ""
    @State private var endpoint = ""
    @State private var region = ""
    @State private var bucket = ""
    @State private var prefix = ""
    @State private var accessKey = ""
    @State private var secretKey = ""
    @State private var folder: String?
    @State private var repository = ""
    @State private var branch = ""
    @State private var token = ""
    @State private var tokenUser = ""
    @State private var picking = false
    @State private var busy = false
    @State private var problem: String?
    /// Another device's vault was there: how to join it.
    @State private var other: (place: Place, passphrase: Bool)?
    @State private var started: String?
    @State private var joinKey = ""
    @State private var joinPassphrase = ""

    var body: some View {
        Form {
            if let started {
                Section {
                    Text(tr("This phone syncs there now. The recovery key opens the vault on your other devices: write it down."))
                        .foregroundStyle(Dusk.label)
                    Text(started)
                        .font(.system(size: 15, design: .monospaced))
                        .textSelection(.enabled)
                    Button(tr("Copy")) { UIPasteboard.general.string = started }
                } header: {
                    SectionHeading(text: tr("Recovery key"))
                }
                .listRowBackground(Dusk.surface)
            } else if let other {
                joinSection(other.place, passphrase: other.passphrase)
            } else {
                placeSection
            }
            if let problem {
                Text(problem).foregroundStyle(Dusk.badText).listRowBackground(Dusk.badBg)
            }
        }
        .scrollContentBackground(.hidden)
        .background(Dusk.panel.ignoresSafeArea())
        .textInputAutocapitalization(.never)
        .autocorrectionDisabled()
        .disabled(busy)
        .overlay { if busy { ProgressView() } }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { result in
            switch result {
            case .success(let url):
                do { folder = try SyncFolder.keep(url) } catch { problem = "\(error)" }
            case .failure(let e):
                problem = e.localizedDescription
            }
        }
    }

    @ViewBuilder
    private var placeSection: some View {
        Section {
            Picker(tr("Where"), selection: $kind) {
                ForEach(Kind.allCases) { Text($0 == .folder ? tr("Folder") : $0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented)
            switch kind {
            case .webdav:
                TextField("https://cloud.example.com/remote.php/dav/files/me/shellwing", text: $url)
                    .keyboardType(.URL)
                TextField(tr("User"), text: $user)
                SecureField(tr("Password or app password"), text: $password)
            case .s3:
                TextField(tr("Endpoint: https://s3.eu-west-1.amazonaws.com"), text: $endpoint).keyboardType(.URL)
                TextField(tr("Region (empty: auto)"), text: $region)
                TextField(tr("Bucket"), text: $bucket)
                TextField(tr("Folder in the bucket (optional)"), text: $prefix)
                TextField(tr("Access key"), text: $accessKey)
                SecureField(tr("Secret key"), text: $secretKey)
            case .git:
                TextField("https://github.com/you/vault.git", text: $repository)
                    .keyboardType(.URL)
                TextField(tr("Branch"), text: $branch)
                SecureField(tr("Token"), text: $token)
                TextField(tr("The token's user"), text: $tokenUser)
            case .folder:
                Button(folder == nil ? tr("Choose a folder…") : tr("Choose another folder…")) { picking = true }
                if let folder {
                    Text(folder).font(.system(size: 12, design: .monospaced)).foregroundStyle(Dusk.muted)
                }
            }
            Button(tr("Connect")) { connect() }
                .disabled(!ready)
        } header: {
            SectionHeading(text: tr("Sync the vault"))
        } footer: {
            Text(footer).foregroundStyle(Dusk.muted)
        }
        .listRowBackground(Dusk.surface)
    }

    @ViewBuilder
    private func joinSection(_ place: Place, passphrase: Bool) -> some View {
        Section {
            Text(tr("Another device's vault is there. Join it to get its hosts, keys, passwords and snippets on this phone."))
                .foregroundStyle(Dusk.label)
            Button(tr("Pair with a device that syncs there")) {
                app.core?.askToJoin(place: place)
                app.refreshSync()
            }
        } header: {
            SectionHeading(text: tr("Join"))
        }
        .listRowBackground(Dusk.surface)
        Section {
            TextField(tr("XXXX-XXXX-…, 13 groups of 4"), text: $joinKey)
                .font(.system(.body, design: .monospaced))
            Button(tr("Join with the recovery key")) {
                run { try $0.joinWithRecoveryKey(place: place, key: joinKey) }
            }
            .disabled(joinKey.count < 20)
        } header: {
            SectionHeading(text: tr("Recovery key"))
        }
        .listRowBackground(Dusk.surface)
        if passphrase {
            Section {
                SecureField(tr("The passphrase set on the other device"), text: $joinPassphrase)
                Button(tr("Join with the passphrase")) {
                    run { try $0.joinWithPassphrase(place: place, passphrase: joinPassphrase) }
                }
                .disabled(joinPassphrase.isEmpty)
            } header: {
                SectionHeading(text: tr("Passphrase"))
            }
            .listRowBackground(Dusk.surface)
        }
    }

    private var footer: String {
        switch kind {
        case .folder: return tr("A folder another device syncs in too, such as shellwing's in iCloud Drive.")
        case .git:
            return tr("A private repository on GitHub, GitLab, Gitea or your own server, by its https:// address, with a token that may push there (on GitHub, a fine-grained token with Contents: read and write). Each change is a commit. The token is kept in this phone's vault.")
        default: return tr("The password is kept in this phone's vault, never in the place.")
        }
    }

    private var ready: Bool {
        switch kind {
        case .webdav: return !url.isEmpty
        case .git: return repository.hasPrefix("https://") && !token.isEmpty
        case .s3: return !endpoint.isEmpty && !bucket.isEmpty && !accessKey.isEmpty && !secretKey.isEmpty
        case .folder: return folder != nil
        }
    }

    private var place: (Place, String?) {
        switch kind {
        case .webdav:
            return (.webDav(url: url.trimmingCharacters(in: .whitespaces), user: user), password.isEmpty ? nil : password)
        case .s3:
            return (.s3(endpoint: endpoint, region: region, bucket: bucket, prefix: prefix, accessKey: accessKey), secretKey)
        case .git:
            let trim = { (s: String) in s.trimmingCharacters(in: .whitespaces) }
            return (.git(url: trim(repository), branch: trim(branch), user: trim(tokenUser)), token)
        case .folder:
            return (.folder(path: folder ?? ""), nil)
        }
    }

    private func connect() {
        let (p, secret) = place
        busy = true
        problem = nil
        let core = app.core
        Task.detached {
            do {
                guard let core else { throw MobileError.Failed(tr("shellwing did not start.")) }
                let found = try core.connectSync(place: p, secret: secret)
                await MainActor.run {
                    busy = false
                    switch found {
                    case .started(let key): started = key
                    case .other(let passphrase): other = (p, passphrase)
                    }
                    app.refreshSync()
                }
            } catch {
                await MainActor.run {
                    busy = false
                    problem = "\(error)"
                }
            }
        }
    }

    /// A join that reaches the place: off the main thread.
    private func run(_ join: @escaping @Sendable (Shellwing) throws -> Void) {
        busy = true
        problem = nil
        let core = app.core
        Task.detached {
            do {
                guard let core else { throw MobileError.Failed(tr("shellwing did not start.")) }
                try join(core)
                await MainActor.run {
                    busy = false
                    app.refreshSync()
                }
            } catch {
                await MainActor.run {
                    busy = false
                    problem = "\(error)"
                }
            }
        }
    }
}
