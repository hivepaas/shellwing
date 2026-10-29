import Foundation
import Network
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
    /// The language spoken, by tag: the core's choice, from Settings or the phone.
    @Published private(set) var language = languageInUse()
    private let syncHears = SyncHears()
    /// Time iOS gives the app in the background, while sessions are open.
    private var background: UIBackgroundTaskIdentifier = .invalid
    /// The network, watched: a change (Wi-Fi to cellular) checks the sessions.
    private let paths = NWPathMonitor()
    private var lastPath: String?

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
        language = languageInUse()
        syncHears.app = self
        core?.setSyncListener(listener: syncHears)
        paths.pathUpdateHandler = { [weak self] path in
            let now = path.status == .satisfied
                ? path.availableInterfaces.map(\.name).joined(separator: ",")
                : "none"
            DispatchQueue.main.async { self?.networkMoved(now) }
        }
        paths.start(queue: DispatchQueue(label: "shellwing.network"))
        reload()
        refreshSync()
    }

    /// Speaks `tag` (nil: the phone's language) from now on.
    func setLanguage(_ tag: String?) {
        do {
            try core?.setLanguage(tag: tag)
        } catch {
            self.error = "\(error)"
        }
        language = languageInUse()
        reload()
    }

    /// The sync's state, read again; a sync done may have brought hosts.
    func refreshSync() {
        guard let core else { return }
        let now = core.syncStatus()
        if now.last != sync?.last { reload() }
        sync = now
    }

    // The app in the background, and back. iOS suspends an app moments
    // after it leaves the screen, and a suspended app's connections die
    // unheard: they are checked when it comes back, and made again
    // (`MobileSession.resumed`, as the setting says).

    /// Gone to the background: the time iOS gives (about half a minute) is
    /// asked for, so a quick look at another app finds the sessions as they
    /// were, and a transfer finishes.
    func wentAway() {
        guard background == .invalid,
              sessions.contains(where: { $0.status == .connected || $0.status == .connecting }) else { return }
        background = UIApplication.shared.beginBackgroundTask(withName: "shellwing sessions") { [weak self] in
            self?.endBackground()
        }
    }

    /// Back: the sessions checked.
    func cameBack() {
        endBackground()
        for s in sessions { s.resumed() }
    }

    private func endBackground() {
        guard background != .invalid else { return }
        UIApplication.shared.endBackgroundTask(background)
        background = .invalid
    }

    /// The network moved (another interface, or back after none): the
    /// sessions checked, once it is there.
    private func networkMoved(_ now: String) {
        defer { lastPath = now }
        guard let was = lastPath, was != now, now != "none" else { return }
        for s in sessions { s.resumed() }
    }

    /// What a session does when its connection ends (the desktop's setting).
    var onDisconnect: OnDisconnectWay {
        core?.settings().onDisconnect ?? .reconnect
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

    /// Saves the host: its id, or why not.
    func trySave(_ edit: HostEdit) throws -> String {
        guard let core else { throw MobileError.Failed(problem ?? "") }
        let id = try core.saveHost(edit: edit)
        reload()
        return id
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
            // Config files left on the host by a session that could not take
            // them away (the app was stopped): tidied once it connects.
            model.connected = { [weak model] in
                Task { @MainActor in
                    if let said = await core.tidyPlaced(session: session) { model?.notice = said }
                }
            }
            sessions.append(model)
            return model
        } catch {
            self.error = "\(error)"
            return nil
        }
    }

    /// Closes a session: the config files it placed until it ends go first
    /// (those it cannot take away are tidied when the host connects next).
    func close(_ model: SessionModel) {
        sessions.removeAll { $0 === model }
        guard let core, let handle = model.handle, !model.placed.isEmpty else { return }
        Task { try? await core.removePlaced(session: handle, path: nil) }
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
        guard let core, let handle = session.handle else { return tr("Not connected.") }
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

    /// Environment `id`'s text with the values it keeps shown in it. The
    /// editor asks it is the user first.
    func envTextShown(id: String, text: String) throws -> EnvShown? {
        try core?.envTextShown(id: id, text: text)
    }

    /// The values shown and not changed kept again (`NAME=`).
    func envTextHidden(id: String, text: String) -> String {
        core?.envTextHidden(id: id, text: text) ?? text
    }

    /// Loads environment `id` into `session`'s shell: nil, or why not.
    func load(env id: String, in session: SessionModel) -> String? {
        guard let core, let handle = session.handle else { return tr("Not connected.") }
        do {
            try core.loadEnv(session: handle, id: id)
            return nil
        } catch {
            return "\(error)"
        }
    }

    /// The notes; with `spec`, those about its host first.
    func notes(for spec: String? = nil) -> [NoteRow] {
        core?.notes(spec: spec) ?? []
    }

    func noteText(_ id: String) -> String {
        (try? core?.noteText(id: id)) ?? ""
    }

    /// Saves a note (`text` nil keeps its text); its id, or why not.
    func saveNote(
        id: String, title: String, category: String, markdown: Bool,
        hosts: [String], sensitive: Bool, text: String?
    ) -> Result<String, Error> {
        guard let core else { return .failure(MobileError.Failed(problem ?? "")) }
        return Result {
            try core.saveNote(
                id: id, title: title, category: category, markdown: markdown,
                hosts: hosts, sensitive: sensitive, text: text
            )
        }
    }

    /// The image `attachment:<id>` names, decoded once and kept.
    func noteImage(_ id: String) -> UIImage? {
        if let seen = images.object(forKey: id as NSString) { return seen }
        guard let bytes = core?.noteImage(id: id), let image = UIImage(data: bytes) else { return nil }
        images.setObject(image, forKey: id as NSString)
        return image
    }

    private let images = NSCache<NSString, UIImage>()

    // Config files: kept in the vault, placed on a session's host.

    func configFiles(for spec: String?) -> [ConfigFileRow] {
        core?.configFiles(spec: spec) ?? []
    }

    func configFileText(_ id: String) -> String? {
        (try? core?.configFileText(id: id)) ?? nil
    }

    /// Saves a config file (`content` nil keeps the one it has): nil, or why not.
    func save(configFile row: ConfigFileRow, content: String?) -> String? {
        guard let core else { return problem }
        do {
            _ = try core.saveConfigFile(row: row, content: content)
            return nil
        } catch {
            return "\(error)"
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
    /// A lost connection about to be tried again: in how long, which try.
    @MainActor @Published private(set) var retrying: Retrying?
    /// Config files placed on the host until the session ends.
    @MainActor @Published private(set) var placed: [PlacedRow] = []
    /// Called on the main thread each time the session connects.
    @MainActor var connected: (() -> Void)?
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
        if s != status {
            status = s
            if s == .connected { connected?() }
        }
        let t = session.title()
        if t != title { title = t }
        let e = session.ended()
        if e != ended { ended = e }
        let o = session.offer()
        if o != offer { offer = o }
        if let n = session.notice() { notice = n }
        // A selection copied: its text, on the pasteboard.
        if let copied = session.copied() { UIPasteboard.general.string = copied }
        let loaded = session.loadedEnvs()
        if loaded != envs { envs = loaded }
        let r = session.retrying()
        if r != retrying { retrying = r }
        refreshPlaced()
    }

    @MainActor
    func refreshPlaced() {
        let now = session?.placed() ?? []
        if now != placed { placed = now }
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

    /// A finger selecting at a point of the grid, in pixels (see `Selecting`).
    func select(_ how: Selecting, x: Float, y: Float) {
        session?.select(how: how, x: x, y: y)
    }

    func clearSelection() {
        session?.clearSelection()
    }

    /// The selection's text goes on the pasteboard once the terminal has it.
    func copySelection() {
        session?.copySelection()
    }

    func reconnect() {
        session?.reconnect()
    }

    @MainActor
    func stopRetrying() {
        session?.stopRetrying()
        retrying = nil
    }

    /// Back from the background, or the network moved: checked, and made
    /// again if it no longer answers.
    func resumed() {
        session?.resumed()
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
        guard let session else { throw MobileError.Failed(tr("Not connected.")) }
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
        case .connected: return tr("connected")
        case .connecting: return tr("connecting")
        case .lost: return tr("connection lost")
        case .failed: return tr("could not connect")
        case .closed: return tr("closed")
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
