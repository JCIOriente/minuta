//! Language codes accepted by whisper.cpp, with English display names.

use crate::error::{Error, Result};

// Source: ggml-org/whisper.cpp, src/whisper.cpp, the `g_lang` map. Whisper stores
// lowercase names; they are shown here in Title Case.
const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("zh", "Chinese"),
    ("de", "German"),
    ("es", "Spanish"),
    ("ru", "Russian"),
    ("ko", "Korean"),
    ("fr", "French"),
    ("ja", "Japanese"),
    ("pt", "Portuguese"),
    ("tr", "Turkish"),
    ("pl", "Polish"),
    ("ca", "Catalan"),
    ("nl", "Dutch"),
    ("ar", "Arabic"),
    ("sv", "Swedish"),
    ("it", "Italian"),
    ("id", "Indonesian"),
    ("hi", "Hindi"),
    ("fi", "Finnish"),
    ("vi", "Vietnamese"),
    ("he", "Hebrew"),
    ("uk", "Ukrainian"),
    ("el", "Greek"),
    ("ms", "Malay"),
    ("cs", "Czech"),
    ("ro", "Romanian"),
    ("da", "Danish"),
    ("hu", "Hungarian"),
    ("ta", "Tamil"),
    ("no", "Norwegian"),
    ("th", "Thai"),
    ("ur", "Urdu"),
    ("hr", "Croatian"),
    ("bg", "Bulgarian"),
    ("lt", "Lithuanian"),
    ("la", "Latin"),
    ("mi", "Maori"),
    ("ml", "Malayalam"),
    ("cy", "Welsh"),
    ("sk", "Slovak"),
    ("te", "Telugu"),
    ("fa", "Persian"),
    ("lv", "Latvian"),
    ("bn", "Bengali"),
    ("sr", "Serbian"),
    ("az", "Azerbaijani"),
    ("sl", "Slovenian"),
    ("kn", "Kannada"),
    ("et", "Estonian"),
    ("mk", "Macedonian"),
    ("br", "Breton"),
    ("eu", "Basque"),
    ("is", "Icelandic"),
    ("hy", "Armenian"),
    ("ne", "Nepali"),
    ("mn", "Mongolian"),
    ("bs", "Bosnian"),
    ("kk", "Kazakh"),
    ("sq", "Albanian"),
    ("sw", "Swahili"),
    ("gl", "Galician"),
    ("mr", "Marathi"),
    ("pa", "Punjabi"),
    ("si", "Sinhala"),
    ("km", "Khmer"),
    ("sn", "Shona"),
    ("yo", "Yoruba"),
    ("so", "Somali"),
    ("af", "Afrikaans"),
    ("oc", "Occitan"),
    ("ka", "Georgian"),
    ("be", "Belarusian"),
    ("tg", "Tajik"),
    ("sd", "Sindhi"),
    ("gu", "Gujarati"),
    ("am", "Amharic"),
    ("yi", "Yiddish"),
    ("lo", "Lao"),
    ("uz", "Uzbek"),
    ("fo", "Faroese"),
    ("ht", "Haitian Creole"),
    ("ps", "Pashto"),
    ("tk", "Turkmen"),
    ("nn", "Nynorsk"),
    ("mt", "Maltese"),
    ("sa", "Sanskrit"),
    ("lb", "Luxembourgish"),
    ("my", "Myanmar"),
    ("bo", "Tibetan"),
    ("tl", "Tagalog"),
    ("mg", "Malagasy"),
    ("as", "Assamese"),
    ("tt", "Tatar"),
    ("haw", "Hawaiian"),
    ("ln", "Lingala"),
    ("ha", "Hausa"),
    ("ba", "Bashkir"),
    ("jw", "Javanese"),
    ("su", "Sundanese"),
    ("yue", "Cantonese"),
];

/// English name of a whisper language code, e.g. `"es"` -> `Some("Spanish")`.
pub fn name(code: &str) -> Option<&'static str> {
    LANGUAGES.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

/// Like [`name`], but an unknown code is a usage error naming the code.
pub fn validate(code: &str) -> Result<&'static str> {
    name(code).ok_or_else(|| {
        Error::Usage(format!(
            "unknown language code '{code}' (use a whisper.cpp code such as en, es, pt)"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_name_known_codes() {
        assert_eq!(name("en"), Some("English"));
        assert_eq!(name("es"), Some("Spanish"));
        assert_eq!(name("pt"), Some("Portuguese"));
        assert_eq!(name("de"), Some("German"));
        assert_eq!(name("fr"), Some("French"));
        assert_eq!(name("ja"), Some("Japanese"));
        assert_eq!(name("zh"), Some("Chinese"));
        assert_eq!(name("yue"), Some("Cantonese"));
        assert_eq!(name("haw"), Some("Hawaiian"));
    }

    #[test]
    fn lang_name_unknown_is_none() {
        assert_eq!(name("xx"), None);
    }

    #[test]
    fn lang_table_has_no_duplicate_codes() {
        for (i, (c, _)) in LANGUAGES.iter().enumerate() {
            assert!(LANGUAGES[i + 1..].iter().all(|(d, _)| d != c), "{c}");
        }
    }

    #[test]
    fn lang_validate_known_returns_name() {
        assert_eq!(validate("es").unwrap(), "Spanish");
    }

    #[test]
    fn lang_validate_unknown_is_usage() {
        let err = validate("xx").unwrap_err();
        assert!(matches!(err, Error::Usage(ref m) if m.contains("xx")));
    }
}
