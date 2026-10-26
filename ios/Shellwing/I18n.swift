import Foundation

// The app's words, in the user's language: the desktop's catalogs, through
// the core (`crates/mobile/src/i18n.rs`). The English is said here, with
// `{name}` for each value; `scripts/i18n.py` finds every `tr("…")` and
// `trN(n, "…", "…")` to keep the translations in step.

/// `english` in the language in use, its `{name}`s filled from `args`.
func tr(_ english: String, _ args: [String: String] = [:]) -> String {
    tr(english: english, args: args)
}

/// A message that says a count, `{n}`: English's two forms.
func trN(_ n: Int, _ one: String, _ other: String, _ args: [String: String] = [:]) -> String {
    trN(n: Int64(n), one: one, other: other, args: args)
}

/// The language spoken, by tag (`en`, `vi`).
func languageInUse() -> String {
    language()
}
