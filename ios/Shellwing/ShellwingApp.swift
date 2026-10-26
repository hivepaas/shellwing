import SwiftUI

@main
struct ShellwingApp: App {
    @StateObject private var app = AppModel()
    @StateObject private var lock = AppLock()
    @Environment(\.scenePhase) private var phase
    /// `-tab sessions` at launch (debug builds) starts on that tab.
    @State private var tab = UserDefaults.standard.string(forKey: "tab") ?? "hosts"

    init() {
        UITabBar.appearance().unselectedItemTintColor = UIColor(hex: 0xa0aab7)
    }

    var body: some Scene {
        WindowGroup {
            ZStack {
                TabView(selection: $tab) {
                    HostsView()
                        .tabItem { Label("Hosts", systemImage: "server.rack") }
                        .tag("hosts")
                    SessionsView()
                        .tabItem { Label("Sessions", systemImage: "terminal") }
                        .badge(app.sessions.count)
                        .tag("sessions")
                    LibraryView()
                        .tabItem { Label("Library", systemImage: "books.vertical") }
                        .tag("library")
                    SettingsView()
                        .tabItem { Label("Settings", systemImage: "gearshape") }
                        .tag("settings")
                }
                .tint(Dusk.accent)
                if lock.locked {
                    LockView(lock: lock, sessions: app.sessions.count)
                        .transition(.opacity)
                }
            }
            .environmentObject(app)
            .environmentObject(lock)
            #if DEBUG
            // `-open <host>` at launch: the session opens whichever tab shows.
            .task {
                if let spec = UserDefaults.standard.string(forKey: "open") {
                    _ = app.session(for: spec)
                }
            }
            #endif
            .preferredColorScheme(.dark)
            .onChange(of: phase) { _, now in
                switch now {
                case .background: lock.wentAway()
                case .active: lock.cameBack()
                default: break
                }
            }
        }
    }
}
