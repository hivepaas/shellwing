//! shellwing's words in the user's language.
//!
//! The English is in the code, where it is said: `tr!("Sync now")`,
//! `tr!("Cannot sync: {why}", why = e)`, `tr_n!(n, "{n} host", "{n} hosts")`.
//! A translation is found by the English it translates: each message's id
//! is a hash of its English text (see [`id_of`]), and `locales/<tag>.ftl`
//! holds the translations under those ids, in Fluent, with the English as a
//! comment above each (`scripts/i18n.py` writes and checks them). What has no
//! translation yet, and all of English, is shown as written in the code.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};

/// The languages there are, by tag, each named in itself.
pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("fr", "Français"),
    ("pt", "Português (Brasil)"),
    ("vi", "Tiếng Việt"),
    ("ru", "Русский"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("zh", "简体中文"),
];

/// The translations, by language tag (English is the code's own). `pt` is
/// Brazil's Portuguese, `zh` simplified Chinese.
const CATALOGS: &[(&str, &str)] = &[
    ("de", include_str!("../locales/de.ftl")),
    ("es", include_str!("../locales/es.ftl")),
    ("fr", include_str!("../locales/fr.ftl")),
    ("ja", include_str!("../locales/ja.ftl")),
    ("ko", include_str!("../locales/ko.ftl")),
    ("pt", include_str!("../locales/pt.ftl")),
    ("ru", include_str!("../locales/ru.ftl")),
    ("vi", include_str!("../locales/vi.ftl")),
    ("zh", include_str!("../locales/zh.ftl")),
];

/// The language in use, and its translations.
struct Current {
    tag: &'static str,
    bundle: Option<FluentBundle<FluentResource>>,
}

static CURRENT: RwLock<Option<Arc<Current>>> = RwLock::new(None);

/// A value a message says: a name, a path, a number.
pub enum Arg {
    Text(String),
    Number(f64),
}

macro_rules! text_arg {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg {
            fn from(v: $t) -> Self {
                Arg::Text(v.to_string())
            }
        }
    )*};
}
text_arg!(&str, String, &String, char);

macro_rules! number_arg {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg {
            fn from(v: $t) -> Self {
                Arg::Number(v as f64)
            }
        }
    )*};
}
number_arg!(i32, i64, u8, u16, u32, u64, usize, f32, f64);

impl std::fmt::Display for Arg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Arg::Text(s) => f.write_str(s),
            Arg::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => write!(f, "{}", *n as i64),
            Arg::Number(n) => write!(f, "{n}"),
        }
    }
}

/// The English, in the user's language: `tr!("Sync now")`, or with values
/// for its `{name}`s: `tr!("Cannot sync: {why}", why = e)`.
#[macro_export]
macro_rules! tr {
    ($text:literal $(,)?) => {
        $crate::tr($text, &[])
    };
    ($text:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::tr($text, &[$((stringify!($name), $crate::Arg::from($value))),+])
    };
}

/// A message that says a count: English's two forms, the count as `{n}`;
/// translations take as many forms as their language has.
/// `tr_n!(hosts, "{n} host", "{n} hosts")`.
#[macro_export]
macro_rules! tr_n {
    ($n:expr, $one:literal, $other:literal $(,)?) => {
        $crate::tr_n($n as i64, $one, $other, &[])
    };
    ($n:expr, $one:literal, $other:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::tr_n(
            $n as i64,
            $one,
            $other,
            &[$((stringify!($name), $crate::Arg::from($value))),+],
        )
    };
}

/// A message's id: `t` and the FNV-1a hash of its English. A count's is of
/// both its forms, joined by a newline.
pub fn id_of(english: &str) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for b in english.bytes() {
        hash = (hash ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    format!("t{hash:08x}")
}

/// The language tag the system asks for, among [`LANGUAGES`] (English if
/// none of them).
pub fn system_language() -> &'static str {
    sys_locale::get_locales()
        .find_map(|l| supported(&l))
        .unwrap_or("en")
}

/// `tag` (`vi`, `vi-VN`, `pt_BR`) as one of [`LANGUAGES`], if it is one.
fn supported(tag: &str) -> Option<&'static str> {
    let main = tag
        .split(['-', '_', '.'])
        .next()
        .unwrap_or("")
        .to_lowercase();
    LANGUAGES.iter().map(|(t, _)| *t).find(|t| *t == main)
}

/// Uses `choice` (a tag; None or `auto`: the system's). Views say it when
/// drawn next.
pub fn set_language(choice: Option<&str>) {
    let tag = match choice.map(str::trim) {
        None | Some("") | Some("auto") => system_language(),
        Some(t) => supported(t).unwrap_or("en"),
    };
    if language() == tag && CURRENT.read().is_ok_and(|c| c.is_some()) {
        return;
    }
    let bundle = CATALOGS
        .iter()
        .find(|(t, _)| *t == tag)
        .map(|(t, source)| bundle_of(t, source));
    if let Ok(mut c) = CURRENT.write() {
        *c = Some(Arc::new(Current { tag, bundle }));
    }
}

fn bundle_of(tag: &str, source: &str) -> FluentBundle<FluentResource> {
    let lang: unic_langid::LanguageIdentifier = tag.parse().unwrap_or_default();
    let mut bundle = FluentBundle::new_concurrent(vec![lang]);
    // No Unicode isolation marks around values: the text is drawn as is.
    bundle.set_use_isolating(false);
    // A broken entry loses only itself: the rest of the file still reads.
    let resource = match FluentResource::try_new(source.to_string()) {
        Ok(r) => r,
        Err((r, _)) => r,
    };
    let _ = bundle.add_resource(resource);
    bundle
}

/// The language in use.
pub fn language() -> &'static str {
    current().map(|c| c.tag).unwrap_or("en")
}

fn current() -> Option<Arc<Current>> {
    CURRENT.read().ok().and_then(|c| c.clone())
}

/// [`tr!`]'s work: `english` in the language in use, its `{name}`s filled.
pub fn tr(english: &str, args: &[(&str, Arg)]) -> String {
    match translated(&id_of(english), args) {
        Some(t) => t,
        None => fill(english, args),
    }
}

/// [`tr_n!`]'s work.
pub fn tr_n(n: i64, one: &str, other: &str, args: &[(&str, Arg)]) -> String {
    let mut all: Vec<(&str, Arg)> = vec![("n", Arg::Number(n as f64))];
    all.extend(args.iter().map(|(k, v)| {
        (
            *k,
            match v {
                Arg::Text(s) => Arg::Text(s.clone()),
                Arg::Number(n) => Arg::Number(*n),
            },
        )
    }));
    let id = id_of(&format!("{one}\n{other}"));
    match translated(&id, &all) {
        Some(t) => t,
        None => fill(if n == 1 { one } else { other }, &all),
    }
}

/// The translation of message `id`, if the language in use has one.
fn translated(id: &str, args: &[(&str, Arg)]) -> Option<String> {
    let current = current()?;
    let bundle = current.bundle.as_ref()?;
    let pattern = bundle.get_message(id)?.value()?;
    let mut fluent = FluentArgs::new();
    for (k, v) in args {
        fluent.set(
            *k,
            match v {
                Arg::Text(s) => FluentValue::from(s.clone()),
                Arg::Number(n) => FluentValue::from(*n),
            },
        );
    }
    let mut errors = vec![];
    let text = bundle.format_pattern(pattern, Some(&fluent), &mut errors);
    Some(text.into_owned())
}

/// `english` with its `{name}`s replaced by their values.
fn fill(english: &str, args: &[(&str, Arg)]) -> String {
    if args.is_empty() || !english.contains('{') {
        return english.to_string();
    }
    let values: HashMap<&str, &Arg> = args.iter().map(|(k, v)| (*k, v)).collect();
    let mut out = String::with_capacity(english.len() + 16);
    let mut rest = english;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if values.contains_key(&after[..close]) => {
                out.push_str(&values[&after[..close]].to_string());
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // The language is one for the whole process: these tests set it in turn.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn english_is_said_as_written_with_its_values() {
        let _turn = ONE_AT_A_TIME.lock();
        set_language(Some("en"));
        assert_eq!(tr!("Sync now"), "Sync now");
        assert_eq!(
            tr!("Cannot sync: {why}.", why = "offline"),
            "Cannot sync: offline."
        );
        assert_eq!(tr!("{a} and {b}", a = 1, b = "x"), "1 and x");
        assert_eq!(tr!("{not an arg} stays", x = 1), "{not an arg} stays");
        assert_eq!(tr_n!(1, "{n} host", "{n} hosts"), "1 host");
        assert_eq!(tr_n!(3, "{n} host", "{n} hosts"), "3 hosts");
    }

    #[test]
    fn a_translation_is_found_by_its_english() {
        let _turn = ONE_AT_A_TIME.lock();
        let source = format!(
            "{} = Đồng bộ ngay\n{} = Không đồng bộ được: {{ $why }}.\n{} = {{ $n }} máy chủ\n",
            id_of("Sync now"),
            id_of("Cannot sync: {why}."),
            id_of("{n} host\n{n} hosts"),
        );
        let bundle = bundle_of("vi", &source);
        if let Ok(mut c) = CURRENT.write() {
            *c = Some(Arc::new(Current {
                tag: "vi",
                bundle: Some(bundle),
            }));
        }
        assert_eq!(tr!("Sync now"), "Đồng bộ ngay");
        assert_eq!(
            tr!("Cannot sync: {why}.", why = "mất mạng"),
            "Không đồng bộ được: mất mạng."
        );
        assert_eq!(tr_n!(3, "{n} host", "{n} hosts"), "3 máy chủ");
        assert_eq!(tr!("Not translated yet"), "Not translated yet");
        set_language(Some("en"));
    }

    #[test]
    fn every_catalog_reads_whole_and_says_its_values() {
        for (tag, source) in CATALOGS {
            if let Err((_, errors)) = FluentResource::try_new(source.to_string()) {
                panic!("{tag}.ftl: {errors:?}");
            }
        }
        let _turn = ONE_AT_A_TIME.lock();
        // Russian's three forms that show, by the count.
        set_language(Some("ru"));
        let hosts = |n: i64| tr_n!(n, "{n} host", "{n} hosts");
        assert_ne!(hosts(1), hosts(3));
        assert_ne!(hosts(3), hosts(5));
        assert_eq!(hosts(21), hosts(1).replace('1', "21"));
        // Every language says every message whole: no `{` left of Fluent's.
        for (tag, _) in CATALOGS {
            set_language(Some(tag));
            for n in [1, 2, 5] {
                let said = tr_n!(n, "{n} host", "{n} hosts");
                assert!(said.contains(&n.to_string()), "{tag}: {said}");
            }
            let said = tr!("Sync: {e}", e = "x");
            assert!(said.contains('x') && !said.contains('$'), "{tag}: {said}");
        }
        set_language(Some("vi"));
        assert_eq!(tr!("Sync now"), "Đồng bộ ngay");
        assert_eq!(tr_n!(3, "{n} host", "{n} hosts"), "3 máy chủ");
        assert_eq!(
            tr!("Sync: {e}", e = "offline"),
            "Đồng bộ: offline",
            "a value is said in place"
        );
        set_language(Some("en"));
    }

    #[test]
    fn languages_are_matched_by_their_main_tag() {
        assert_eq!(supported("vi-VN"), Some("vi"));
        assert_eq!(supported("vi_VN.UTF-8"), Some("vi"));
        assert_eq!(supported("EN-us"), Some("en"));
        assert_eq!(supported("tlh"), None);
        assert_eq!(id_of("Sync now"), id_of("Sync now"));
        assert_ne!(id_of("Sync now"), id_of("Sync now."));
    }
}
