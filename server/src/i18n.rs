//! Language of the messages the server writes for people: API errors, the activity feed, the
//! connection and synchronisation states shown by the web UI and the app.
//!
//! A message is a [`Text`]: its French, English and Spanish versions, built with [`text!`]. It is
//! serialized in the language of the current request (`Accept-Language`, see [`middleware`]) or
//! of the WebSocket (see [`scope`]); English outside of any request. The activity feed stores the
//! three versions, so each reader gets their own.

mod catalog;

use axum::extract::Request;
use axum::http::header::ACCEPT_LANGUAGE;
use axum::middleware::Next;
use axum::response::Response;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::Display;
use std::future::Future;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Fr,
    En,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Fr, Lang::En, Lang::Es];

    /// `fr`, `fr-FR`, `EN_us`… → the language, if supported.
    pub fn parse(code: &str) -> Option<Lang> {
        let primary = code.trim().split(['-', '_']).next()?.to_ascii_lowercase();
        match primary.as_str() {
            "fr" => Some(Lang::Fr),
            "en" => Some(Lang::En),
            "es" => Some(Lang::Es),
            _ => None,
        }
    }

    /// The preferred supported language of an `Accept-Language` header (English otherwise).
    pub fn from_accept_language(header: &str) -> Lang {
        let mut ranges: Vec<(f32, Lang)> = header
            .split(',')
            .filter_map(|range| {
                let mut parts = range.split(';');
                let lang = Lang::parse(parts.next()?)?;
                let q = parts.find_map(|p| p.trim().strip_prefix("q=")?.parse::<f32>().ok()).unwrap_or(1.0);
                Some((q, lang))
            })
            .collect();
        // Stable sort: equal weights keep the order of the header.
        ranges.sort_by(|a, b| b.0.total_cmp(&a.0));
        ranges.first().map_or(Lang::En, |(_, lang)| *lang)
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
            Lang::Es => "es",
        }
    }
}

tokio::task_local! {
    static LANG: Lang;
}

/// Language of the current request or WebSocket; English outside of them (background tasks).
pub fn current() -> Lang {
    LANG.try_with(|l| *l).unwrap_or(Lang::En)
}

/// Runs `fut` with `lang` as the current language.
pub async fn scope<F: Future>(lang: Lang, fut: F) -> F::Output {
    LANG.scope(lang, fut).await
}

/// Axum middleware: the language of the request comes from its `Accept-Language` header.
pub async fn middleware(req: Request, next: Next) -> Response {
    let lang = req.headers().get(ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok()).map_or(Lang::En, Lang::from_accept_language);
    scope(lang, next.run(req)).await
}

/// A message for people, in every supported language.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Text {
    pub fr: String,
    pub en: String,
    pub es: String,
}

impl Text {
    /// Builds a message from a function giving it in a language.
    pub fn build(f: impl Fn(Lang) -> String) -> Text {
        Text { fr: f(Lang::Fr), en: f(Lang::En), es: f(Lang::Es) }
    }

    /// The same words in every language (a name, an error coming from elsewhere).
    pub fn plain(s: impl Into<String>) -> Text {
        let s = s.into();
        Text { fr: s.clone(), en: s.clone(), es: s }
    }

    pub fn get(&self, lang: Lang) -> &str {
        match lang {
            Lang::Fr => &self.fr,
            Lang::En => &self.en,
            Lang::Es => &self.es,
        }
    }

    /// In the current language.
    pub fn current(&self) -> String {
        self.get(current()).to_string()
    }
}

impl Text {
    /// The three versions as JSON, to store a message in the database.
    pub fn store(&self) -> String {
        serde_json::json!({ "fr": self.fr, "en": self.en, "es": self.es }).to_string()
    }

    /// Reads back [`Text::store`]; anything else (an older plain message) is the same in every language.
    pub fn load(stored: &str) -> Text {
        let parsed: Option<serde_json::Value> = serde_json::from_str(stored).ok();
        match parsed.as_ref().and_then(|v| Some((v["fr"].as_str()?, v["en"].as_str()?, v["es"].as_str()?))) {
            Some((fr, en, es)) => Text { fr: fr.into(), en: en.into(), es: es.into() },
            None => Text::plain(stored),
        }
    }
}

impl From<&str> for Text {
    /// A fixed message of the code: its translations if known, the same words otherwise.
    fn from(s: &str) -> Text {
        catalog::lookup(s).unwrap_or_else(|| Text::plain(s))
    }
}

impl From<String> for Text {
    fn from(s: String) -> Text {
        catalog::lookup(&s).unwrap_or_else(|| Text::plain(s))
    }
}

impl From<&String> for Text {
    fn from(s: &String) -> Text {
        Text::from(s.as_str())
    }
}

/// Serialized as a string, in the current language.
impl Serialize for Text {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.get(current()))
    }
}

/// Read back as the same words in every language (it came from elsewhere, already translated).
impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Text, D::Error> {
        String::deserialize(d).map(Text::plain)
    }
}

/// An argument of [`text!`]: a [`Text`] gives its version in each language, anything else is
/// written as it is.
pub trait Localize {
    fn localize(&self, lang: Lang) -> String;
}

impl<T: Display + ?Sized> Localize for T {
    fn localize(&self, _: Lang) -> String {
        self.to_string()
    }
}

impl Text {
    /// This message as a [`text!`] argument, translated along with the sentence around it.
    pub fn arg(&self) -> TextArg<'_> {
        TextArg(self)
    }
}

/// A [`Text`] passed to [`text!`].
pub struct TextArg<'a>(&'a Text);

impl TextArg<'_> {
    pub fn localize(&self, lang: Lang) -> String {
        self.0.get(lang).to_string()
    }
}

/// `text!("fr {0}", "en {0}", "es {0}", arg…)` → a [`Text`]. Arguments are positional (`{0}`,
/// `{1}`…) and must be plain expressions (they are read once per language); a [`Text`] argument
/// is passed as `t.arg()` to be translated along with the sentence.
#[macro_export]
macro_rules! text {
    ($fr:literal, $en:literal, $es:literal $(,)?) => {
        $crate::i18n::Text { fr: format!($fr), en: format!($en), es: format!($es) }
    };
    ($fr:literal, $en:literal, $es:literal, $($arg:expr),+ $(,)?) => {{
        #[allow(unused_imports)]
        use $crate::i18n::Localize as _;
        $crate::i18n::Text::build(|lang| match lang {
            $crate::i18n::Lang::Fr => format!($fr, $(($arg).localize(lang)),+),
            $crate::i18n::Lang::En => format!($en, $(($arg).localize(lang)),+),
            $crate::i18n::Lang::Es => format!($es, $(($arg).localize(lang)),+),
        })
    }};
}

/// A message in the current language: `tr!("fr", "en", "es", args…)`.
#[macro_export]
macro_rules! tr {
    ($($t:tt)*) => {
        $crate::text!($($t)*).current()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_language_picks_the_preferred_supported_language() {
        assert_eq!(Lang::from_accept_language("fr-FR,fr;q=0.9,en;q=0.8"), Lang::Fr);
        assert_eq!(Lang::from_accept_language("de-DE,es;q=0.7,en;q=0.5"), Lang::Es);
        assert_eq!(Lang::from_accept_language("en;q=0.4, fr;q=0.8"), Lang::Fr);
        assert_eq!(Lang::from_accept_language("de, it"), Lang::En);
        assert_eq!(Lang::from_accept_language(""), Lang::En);
    }

    #[tokio::test]
    async fn texts_follow_the_current_language() {
        let n = 3;
        let inner = text!("allumée", "on", "encendida");
        let t = text!("{0} zones : {1}", "{0} zones: {1}", "{0} zonas: {1}", n, inner.arg());
        assert_eq!(t.en, "3 zones: on");
        assert_eq!(t.get(Lang::Es), "3 zonas: encendida");
        assert_eq!(serde_json::to_string(&t).unwrap(), "\"3 zones: on\"", "English outside of a request");
        let fr = scope(Lang::Fr, async { serde_json::to_string(&t).unwrap() }).await;
        assert_eq!(fr, "\"3 zones : allumée\"");
    }
}
