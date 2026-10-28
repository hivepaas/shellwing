package com.hivepaas.shellwing

// The app's words, in the user's language: the desktop's catalogs, through
// the core (crates/mobile/src/i18n.rs). The English is said here, with
// `{name}` for each value; scripts/i18n.py finds every tr("…") and
// trN(n, "…", "…") to keep the translations in step.

/** [english] in the language in use, its `{name}`s filled from [args]. */
fun tr(english: String, vararg args: Pair<String, String>): String =
    com.hivepaas.shellwing.core.tr(english, args.toMap())

/** A message that says a count, `{n}`: English's two forms. */
fun trN(n: Int, one: String, other: String, vararg args: Pair<String, String>): String =
    com.hivepaas.shellwing.core.trN(n.toLong(), one, other, args.toMap())
