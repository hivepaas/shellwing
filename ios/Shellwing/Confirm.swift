import SwiftUI

/// Something about to be deleted or forgotten, asked about first: what
/// goes, what then happens, and the deed.
struct Doomed: Identifiable {
    let id = UUID()
    /// The row it is about: the question shows by that row (on iOS 26, a
    /// popover pointing at it).
    let key: String
    let title: String
    var message: String?
    /// The button that does it: Delete, Forget.
    var action = tr("Delete")
    let run: () -> Void
}

extension View {
    /// On the row `key`: asks there before `doomed` goes, if it is about
    /// that row; nothing happens on Cancel.
    func confirming(_ doomed: Binding<Doomed?>, about key: String) -> some View {
        confirmationDialog(
            doomed.wrappedValue?.title ?? "",
            isPresented: Binding(
                get: { doomed.wrappedValue?.key == key },
                set: { if !$0 { doomed.wrappedValue = nil } }
            ),
            titleVisibility: .visible,
            presenting: doomed.wrappedValue
        ) { d in
            Button(d.action, role: .destructive) { d.run() }
            Button(tr("Cancel"), role: .cancel) {}
        } message: { d in
            if let m = d.message { Text(m) }
        }
    }
}

/// A swipe action that deletes: red, and only asks (`ask`). Not of the
/// destructive role, which would take the row away before the answer.
struct DeleteSwipe: View {
    var label = tr("Delete")
    let ask: () -> Void

    var body: some View {
        Button(label, action: ask).tint(Dusk.danger)
    }
}

/// Said under a vault item about to go: it goes everywhere.
func goesEverywhere() -> String {
    tr("If the vault syncs, it goes from your other devices too.")
}
