import Foundation
import SwiftUI

/// The app's state: shellwing-core, the hosts it lists, and the sessions open.
@MainActor
final class AppModel: ObservableObject {
    let core: Shellwing?
    /// Why the core could not start, if it did not.
    let problem: String?

    @Published private(set) var sections: [HostSection] = []
    @Published private(set) var sessions: [SessionModel] = []
    @Published private(set) var sync: SyncStatus?
    @Published var error: String?
    private let syncHears = SyncHears()

    init() {
        let dir = AppModel.configDir()
        // known_hosts goes beside the configuration: the app's home is not
        // its to write in.
        setenv("SHELLWING_KNOWN_HOSTS", dir.appendingPathComponent("known_hosts").path, 1)
        // A sync folder the user gave (iCloud Drive): open to the app again
        // before the sync worker looks at it.
        SyncFolder.resume()
        do {
            core = try Shellwing(configDir: dir.path, keys: KeychainKeys())
            problem = nil
        } catch {
            core = nil
            problem = "\(error)"
        }
        syncHears.app = self
        core?.setSyncListener(listener: syncHears)
        reload()
        refreshSync()
    }

    /// The sync's state, read again; a sync done may have brought hosts.
    func refreshSync() {
        guard let core else { return }
        let now = core.syncStatus()
        if now.last != sync?.last { reload() }
        sync = now
    }

    /// Application Support/shellwing: kept, backed up, never shown in Files.
    static func configDir() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("shellwing", isDirectory: true)
    }

    func reload() {
        guard let core else { return }
        do {
            sections = try core.sections()
        } catch {
            self.error = "\(error)"
        }
    }

    func groups() -> [String] {
        core?.groups() ?? []
    }

    func host(id: String) -> HostEdit? {
        core?.host(id: id)
    }

    /// Saves the host; its id, or nil and `error` set.
    @discardableResult
    func save(_ edit: HostEdit) -> String? {
        guard let core else { return nil }
        do {
            let id = try core.saveHost(edit: edit)
            reload()
            return id
        } catch {
            self.error = "\(error)"
            return nil
        }
    }

    func delete(hostId: String) {
        guard let core else { return }
        do {
            try core.deleteHost(id: hostId)
            reload()
        } catch {
            self.error = "\(error)"
        }
    }

    /// The session open to `spec`, else a new one.
    func session(for spec: String) -> SessionModel? {
        if let open = sessions.first(where: { $0.spec == spec && $0.status != .closed && $0.status != .failed }) {
            return open
        }
        return open(spec)
    }

    /// A new session to `spec`, even with one open already.
    func open(_ spec: String) -> SessionModel? {
        guard let core else { return nil }
        let model = SessionModel(spec: spec)
        do {
            let session = try core.open(spec: spec, listener: model)
            model.attach(session)
            sessions.append(model)
            return model
        } catch {
            self.error = "\(error)"
            return nil
        }
    }

    func close(_ model: SessionModel) {
        sessions.removeAll { $0 === model }
    }

    // The vault: saved passwords and SSH keys. Opening it waits on its own
    // thread the first time; after, these return at once.

    func vaultState() -> VaultState {
        core?.vaultState() ?? VaultState(ready: false, problem: problem)
    }

    func recoveryKey() -> String? {
        core?.recoveryKey()
    }

    func savedSecrets() -> [SavedSecret] {
        core?.savedSecrets() ?? []
    }

    func keys() -> [KeyRow] {
        core?.keys() ?? []
    }

    /// Forgets a saved password or passphrase, or deletes a key.
    func forget(_ id: String) {
        do {
            try core?.forget(id: id)
        } catch {
            self.error = "\(error)"
        }
    }

    // The library: snippets, environments, notes.

    func snippets(for spec: String?) -> [SnippetRow] {
        core?.snippets(spec: spec) ?? []
    }

    @discardableResult
    func save(snippet: SnippetEdit) -> Bool {
        do {
            _ = try core?.saveSnippet(edit: snippet)
            return true
        } catch {
            self.error = "\(error)"
            return false
        }
    }

    /// Runs snippet `id` in `session` with the values asked for: nil, or why not.
    func run(snippet id: String, in session: SessionModel, values: [String: String]) -> String? {
        guard let core, let handle = session.handle else { return "Not connected." }
        do {
            try core.runSnippet(session: handle, id: id, values: values)
            return nil
        } catch {
            return "\(error)"
        }
    }

    func environments() -> [EnvRow] {
        core?.environments() ?? []
    }

    @discardableResult
    func saveEnvironment(id: String, name: String, scope: String, dotenv: String) -> Bool {
        do {
            _ = try core?.saveEnvironment(id: id, name: name, scope: scope, dotenv: dotenv)
            return true
        } catch {
            self.error = "\(error)"
            return false
        }
    }

    /// Loads environment `id` into `session`'s shell: nil, or why not.
    func load(env id: String, in session: SessionModel) -> String? {
        guard let core, let handle = session.handle else { return "Not connected." }
        do {
            try core.loadEnv(session: handle, id: id)
            return nil
        } catch {
            return "\(error)"
        }
    }

    func notes() -> [NoteRow] {
        core?.notes() ?? []
    }

    func noteText(_ id: String) -> String {
        (try? core?.noteText(id: id)) ?? ""
    }

    /// Saves a note; its id, or nil and `error` set.
    func saveNote(id: String, title: String, category: String, markdown: Bool, text: String) -> String? {
        do {
            return try core?.saveNote(id: id, title: title, category: category, markdown: markdown, text: text)
        } catch {
            self.error = "\(error)"
            return nil
        }
    }

    func generateKey(name: String) -> KeyRow? {
        do {
            return try core?.generateKey(name: name)
        } catch {
            self.error = "\(error)"
            return nil
        }
    }
}

/// One session: its latest frame, status and title, published for the views.
/// The Rust side calls `changed()` from its own threads; reading happens here,
/// on the main one, once per wake.
final class SessionModel: ObservableObject, SessionListener, Identifiable, @unchecked Sendable {
    let id = UUID()
    let spec: String
    private var session: MobileSession?
    /// For what runs through the core (snippets, environments).
    var handle: MobileSession? { session }

    @MainActor @Published private(set) var frame: FrameData?
    @MainActor @Published private(set) var status: SessionStatus = .connecting
    @MainActor @Published private(set) var title: String
    @MainActor @Published private(set) var ended: String?
    /// A password that worked, offered to keep: `the password for deploy@web-1`.
    @MainActor @Published private(set) var offer: String?
    /// Something said once: an environment loaded, a load given up.
    @MainActor @Published var notice: String?
    @MainActor @Published private(set) var envs: [LoadedEnvRow] = []
    /// Ctrl or Alt, pressed on the row above the keyboard: held for the next key.
    @MainActor @Published var ctrl = false
    @MainActor @Published var alt = false

    private let pending = NSLock()
    private var scheduled = false

    init(spec: String) {
        self.spec = spec
        self._title = Published(initialValue: spec)
    }

    func attach(_ session: MobileSession) {
        self.session = session
        changed()
    }

    // SessionListener: from Rust's threads. One refresh at a time is enough.
    func changed() {
        pending.lock()
        if scheduled {
            pending.unlock()
            return
        }
        scheduled = true
        pending.unlock()
        DispatchQueue.main.async { [weak self] in
            self?.refresh()
        }
    }

    @MainActor
    private func refresh() {
        pending.lock()
        scheduled = false
        pending.unlock()
        guard let session else { return }
        if let f = session.frame(), f.seq != frame?.seq {
            frame = f
        }
        let s = session.status()
        if s != status { status = s }
        let t = session.title()
        if t != title { title = t }
        let e = session.ended()
        if e != ended { ended = e }
        let o = session.offer()
        if o != offer { offer = o }
        if let n = session.notice() { notice = n }
        let loaded = session.loadedEnvs()
        if loaded != envs { envs = loaded }
    }

    // What the keyboard and the keys row send.

    @MainActor
    func type(_ text: String) {
        guard let session else { return }
        // Ctrl or Alt held from the keys row: a letter goes with it.
        if (ctrl || alt), text.count == 1, let c = text.first, c.isLetter, c.isASCII {
            session.key(key: .letter(letter: String(c)), ctrl: ctrl, alt: alt, shift: false)
            ctrl = false
            alt = false
            return
        }
        if text == "\n" {
            press(.enter)
            return
        }
        session.text(text: text)
        ctrl = false
        alt = false
    }

    @MainActor
    func press(_ key: Key) {
        session?.key(key: key, ctrl: ctrl, alt: alt, shift: false)
        ctrl = false
        alt = false
    }

    @MainActor
    func paste(_ text: String) {
        session?.paste(text: text)
    }

    func resize(cols: UInt16, rows: UInt16, cellW: UInt32, cellH: UInt32) {
        session?.resize(cols: cols, rows: rows, cellW: cellW, cellH: cellH)
    }

    func scroll(lines: Float) {
        session?.scroll(lines: lines)
    }

    func scrollToBottom() {
        session?.scrollToBottom()
    }

    func reconnect() {
        session?.reconnect()
    }

    @MainActor
    func unload(env id: String?) {
        do {
            try session?.unloadEnv(id: id)
            envs = session?.loadedEnvs() ?? []
        } catch {
            notice = "\(error)"
        }
    }

    @MainActor
    func keepOffer() throws {
        try session?.saveOffer()
        offer = nil
    }

    @MainActor
    func dismissOffer() {
        session?.dismissOffer()
        offer = nil
    }

    /// Puts the public key `line` in the host's authorized_keys: true when added, false
    /// when it was there.
    func install(key line: String) async throws -> Bool {
        guard let session else { throw MobileError.Failed("Not connected.") }
        return try await session.installKey(public: line)
    }
}

extension SessionStatus {
    var color: Color {
        switch self {
        case .connected: return Dusk.ok
        case .connecting: return Dusk.amber
        case .lost: return Dusk.lost
        case .failed: return Dusk.bad
        case .closed: return Dusk.idle
        }
    }

    var said: String {
        switch self {
        case .connected: return "connected"
        case .connecting: return "connecting"
        case .lost: return "connection lost"
        case .failed: return "could not connect"
        case .closed: return "closed"
        }
    }
}

/// Hears the sync worker (off the main thread) and refreshes the app on it.
final class SyncHears: SyncListener, @unchecked Sendable {
    weak var app: AppModel?

    func changed() {
        DispatchQueue.main.async { [weak self] in
            self?.app?.refreshSync()
        }
    }
}
