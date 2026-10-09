//! Interface translations, pluralization, reverse lookup, and completeness validation.

use crate::value::JsonValue;
use sse_codecs::embedded_json::{self, JsonAssetCache};
use sse_core::{Error, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Supported interface languages (code -> native title).
pub const SUPPORTED_LANGUAGES: &[(&str, &str)] = &[
    ("ru", "Русский"),
    ("uk", "Українська"),
    ("en", "English"),
    ("de", "Deutsch"),
    ("fr", "Français"),
    ("it", "Italiano"),
    ("es", "Español"),
    ("pl", "Polski"),
    ("cs", "Čeština"),
    ("pt-BR", "Português (Brasil)"),
    ("tr", "Türkçe"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("zh-CN", "简体中文"),
    ("zh-TW", "繁體中文"),
];

static INSTANCE: OnceLock<I18nService> = OnceLock::new();

macro_rules! declare_embedded_i18n {
    ($cache:ident, $asset:ident, $file:literal) => {
        static $cache: JsonAssetCache = OnceLock::new();
        const $asset: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/", $file));
    };
}

declare_embedded_i18n!(I18N_MESSAGES_CACHE, I18N_MESSAGES_ASSET, "i18n__messages.json.deflate");
declare_embedded_i18n!(I18N_CS_CACHE, I18N_CS_ASSET, "i18n_cs.json.deflate");
declare_embedded_i18n!(I18N_DE_CACHE, I18N_DE_ASSET, "i18n_de.json.deflate");
declare_embedded_i18n!(I18N_EN_CACHE, I18N_EN_ASSET, "i18n_en.json.deflate");
declare_embedded_i18n!(I18N_ES_CACHE, I18N_ES_ASSET, "i18n_es.json.deflate");
declare_embedded_i18n!(I18N_FR_CACHE, I18N_FR_ASSET, "i18n_fr.json.deflate");
declare_embedded_i18n!(I18N_IT_CACHE, I18N_IT_ASSET, "i18n_it.json.deflate");
declare_embedded_i18n!(I18N_JA_CACHE, I18N_JA_ASSET, "i18n_ja.json.deflate");
declare_embedded_i18n!(I18N_KO_CACHE, I18N_KO_ASSET, "i18n_ko.json.deflate");
declare_embedded_i18n!(I18N_PL_CACHE, I18N_PL_ASSET, "i18n_pl.json.deflate");
declare_embedded_i18n!(I18N_PT_BR_CACHE, I18N_PT_BR_ASSET, "i18n_pt-BR.json.deflate");
declare_embedded_i18n!(I18N_RU_CACHE, I18N_RU_ASSET, "i18n_ru.json.deflate");
declare_embedded_i18n!(I18N_TR_CACHE, I18N_TR_ASSET, "i18n_tr.json.deflate");
declare_embedded_i18n!(I18N_UK_CACHE, I18N_UK_ASSET, "i18n_uk.json.deflate");
declare_embedded_i18n!(I18N_ZH_CN_CACHE, I18N_ZH_CN_ASSET, "i18n_zh-CN.json.deflate");
declare_embedded_i18n!(I18N_ZH_TW_CACHE, I18N_ZH_TW_ASSET, "i18n_zh-TW.json.deflate");

/// Translation service for UI strings keyed by Russian source strings.
pub struct I18nService {
    current_lang: Mutex<String>,
    catalogs: Mutex<HashMap<String, Arc<HashMap<String, JsonValue>>>>,
    reverse_catalogs: Mutex<HashMap<String, Arc<ReverseCatalog>>>,
}

struct ReverseCatalog {
    exact: HashMap<String, String>,
    patterns: Vec<(String, String)>, // (prefix/suffix pattern, source)
}

impl Default for I18nService {
    fn default() -> Self {
        Self::new()
    }
}

impl I18nService {
    /// Creates a new I18nService instance.
    #[must_use]
    pub fn new() -> Self {
        let initial_lang = std::env::var("STALKER_EDITOR_LANG")
            .ok()
            .and_then(|s| Self::normalize_language_code(Some(&s)))
            .unwrap_or("ru")
            .to_string();

        Self {
            current_lang: Mutex::new(initial_lang),
            catalogs: Mutex::new(HashMap::new()),
            reverse_catalogs: Mutex::new(HashMap::new()),
        }
    }

    /// Global shared translation service instance.
    #[must_use]
    pub fn instance() -> &'static Self {
        INSTANCE.get_or_init(Self::new)
    }

    /// Currently active language code (e.g. "ru", "en", "zh-CN").
    #[must_use]
    pub fn current_language(&self) -> String {
        self.current_lang
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| "ru".to_string())
    }

    /// Switches the active interface language.
    pub fn set_language(&self, code: &str) {
        let normalized = Self::normalize_language_code(Some(code)).unwrap_or("ru");
        if let Ok(mut lock) = self.current_lang.lock() {
            *lock = normalized.to_string();
        }
    }

    /// Normalizes language tags, handling casing, underscores, and regional aliases.
    #[must_use]
    pub fn normalize_language_code(code: Option<&str>) -> Option<&'static str> {
        let raw = code?.trim();
        if raw.is_empty() {
            return None;
        }

        let clean = raw.replace('_', "-");
        let token = clean.split('.').next()?.split('@').next()?.trim();

        if let Some(&(supported, _)) = SUPPORTED_LANGUAGES.iter().find(|(s, _)| s.eq_ignore_ascii_case(token)) {
            return Some(supported);
        }

        let lower = token.to_ascii_lowercase();
        if lower.starts_with("zh") {
            if lower.ends_with("tw") || lower.ends_with("hk") || lower.ends_with("hant") {
                return Some("zh-TW");
            }
            return Some("zh-CN");
        }
        if lower.starts_with("pt") {
            return Some("pt-BR");
        }

        let base_lang = lower.split('-').next()?;
        SUPPORTED_LANGUAGES
            .iter()
            .find(|(s, _)| s.eq_ignore_ascii_case(base_lang))
            .map(|(s, _)| *s)
    }

    /// Translates text into the active language with positional `{0}`, `{1}` args.
    #[must_use]
    pub fn tr(&self, text: &str, args: &[&dyn std::fmt::Display]) -> String {
        let lang = self.current_language();
        self.tr_in(Some(&lang), text, args)
    }

    /// Translates text into a specific language with positional `{0}`, `{1}` args.
    #[must_use]
    pub fn tr_in(&self, code: Option<&str>, text: &str, args: &[&dyn std::fmt::Display]) -> String {
        if text.is_empty() {
            return String::new();
        }

        let target_lang = Self::normalize_language_code(code).unwrap_or("ru");
        let mut pattern = text.to_string();

        if !target_lang.eq_ignore_ascii_case("ru") {
            if let Ok(catalog) = self.get_catalog(target_lang) {
                if let Some(val) = catalog.get(text).and_then(JsonValue::as_str) {
                    if !val.is_empty() {
                        pattern = val.to_string();
                    }
                } else if !target_lang.eq_ignore_ascii_case("en") {
                    if let Ok(en_catalog) = self.get_catalog("en") {
                        if let Some(val) = en_catalog.get(text).and_then(JsonValue::as_str) {
                            if !val.is_empty() {
                                pattern = val.to_string();
                            }
                        }
                    }
                }
            }
        }

        format_placeholders(&pattern, args)
    }

    /// Plural translation according to CLDR rules for a specific language.
    #[must_use]
    pub fn trn_in(
        &self,
        code: Option<&str>,
        count: i64,
        one: &str,
        few: &str,
        many: &str,
        args: &[&dyn std::fmt::Display],
    ) -> String {
        let target_lang = Self::normalize_language_code(code).unwrap_or("ru");
        let key = format!("{one}|{few}|{many}");
        let mut forms = vec![one.to_string(), few.to_string(), many.to_string()];

        if !target_lang.eq_ignore_ascii_case("ru") {
            if let Ok(catalog) = self.get_catalog(target_lang) {
                if let Some(element) = catalog.get(&key) {
                    match element {
                        JsonValue::Array(arr) => {
                            let loaded: Vec<String> = arr
                                .iter()
                                .filter_map(JsonValue::as_str)
                                .map(|s| s.to_string())
                                .collect();
                            if !loaded.is_empty() {
                                forms = loaded;
                            }
                        }
                        JsonValue::String(s) => {
                            forms = vec![s.clone()];
                        }
                        _ => {}
                    }
                }
            }
        }

        let index = get_plural_index(target_lang, count);
        let chosen = forms
            .get(index.min(forms.len().saturating_sub(1)))
            .cloned()
            .unwrap_or_else(|| one.to_string());

        if args.is_empty() {
            chosen
        } else {
            format_placeholders(&chosen, args)
        }
    }

    /// Maps translated text back to its Russian source string for backend processing.
    #[must_use]
    pub fn source_text(&self, text: &str) -> String {
        let current = self.current_language();
        if text.is_empty() || current.eq_ignore_ascii_case("ru") {
            return text.to_string();
        }

        let rev = self.get_reverse_catalog(&current);
        if let Some(source) = rev.exact.get(text) {
            return source.clone();
        }

        for (pattern, source) in &rev.patterns {
            if let Some(recovered) = match_pattern_and_replace(text, pattern, source) {
                return recovered;
            }
        }

        text.to_string()
    }

    /// Retrieves or loads on demand the translations map for a language code.
    pub fn get_catalog(&self, language_code: &str) -> Result<Arc<HashMap<String, JsonValue>>> {
        let normalized = Self::normalize_language_code(Some(language_code))
            .unwrap_or(language_code)
            .to_string();

        let mut lock = self
            .catalogs
            .lock()
            .map_err(|_| Error::damaged("Catalog lock poisoned"))?;

        if let Some(cached) = lock.get(&normalized) {
            return Ok(cached.clone());
        }

        let raw_json = get_embedded_i18n(&normalized)
            .ok_or_else(|| Error::damaged(format!("No catalog for language '{normalized}'.")))?;

        let value = crate::value::parse_json(raw_json)
            .map_err(|e| Error::damaged(format!("Parsing {normalized} catalog failed: {e}")))?;

        let map = match value {
            JsonValue::Object(obj) => obj.into_iter().collect(),
            _ => HashMap::new(),
        };

        let arc = Arc::new(map);
        lock.insert(normalized, arc.clone());
        Ok(arc)
    }

    fn get_reverse_catalog(&self, language_code: &str) -> Arc<ReverseCatalog> {
        let normalized = Self::normalize_language_code(Some(language_code))
            .unwrap_or(language_code)
            .to_string();

        let mut lock = match self.reverse_catalogs.lock() {
            Ok(l) => l,
            Err(p) => p.into_inner(),
        };

        if let Some(cached) = lock.get(&normalized) {
            return cached.clone();
        }

        let mut exact = HashMap::new();
        let mut patterns = Vec::new();

        if let Ok(catalog) = self.get_catalog(&normalized) {
            for (source, val) in catalog.iter() {
                if let Some(translated) = val.as_str() {
                    if translated.is_empty() {
                        continue;
                    }
                    if !translated.contains('{') {
                        exact.insert(translated.to_string(), source.clone());
                    } else {
                        patterns.push((translated.to_string(), source.clone()));
                    }
                }
            }
        }

        let arc = Arc::new(ReverseCatalog { exact, patterns });
        lock.insert(normalized, arc.clone());
        arc
    }
}

fn get_plural_index(lang: &str, count: i64) -> usize {
    let n = count.unsigned_abs();
    let mod10 = n.checked_rem(10).unwrap_or(0);
    let mod100 = n.checked_rem(100).unwrap_or(0);
    match lang {
        "ru" | "uk" => {
            if mod10 == 1 && mod100 != 11 {
                0
            } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                1
            } else {
                2
            }
        }
        "pl" => {
            if n == 1 {
                0
            } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                1
            } else {
                2
            }
        }
        "cs" => {
            if n == 1 {
                0
            } else if (2..=4).contains(&n) {
                1
            } else {
                2
            }
        }
        "ja" | "ko" | "zh-CN" | "zh-TW" | "tr" => 0,
        "fr" | "pt-BR" => {
            if n == 0 || n == 1 {
                0
            } else {
                1
            }
        }
        _ => {
            if n == 1 {
                0
            } else {
                1
            }
        }
    }
}

fn format_placeholders(pattern: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut result = pattern.to_string();
    for (i, arg) in args.iter().enumerate() {
        let simple_placeholder = format!("{{{i}}}");
        let arg_str = arg.to_string();
        result = result.replace(&simple_placeholder, &arg_str);

        // Also handle format specifiers like {0:X4}
        let prefix = format!("{{{i}:");
        while let Some(start) = result.find(&prefix) {
            if let Some(end_offset) = result.get(start..).and_then(|s| s.find('}')) {
                let end = start.saturating_add(end_offset);
                result.replace_range(start..=end, &arg_str);
            } else {
                break;
            }
        }
    }
    result
}

fn match_pattern_and_replace(text: &str, pattern: &str, source: &str) -> Option<String> {
    let placeholder = "{0}";
    let idx = pattern.find(placeholder)?;
    let prefix = pattern.get(..idx)?;
    let suffix_start = idx.saturating_add(placeholder.len());
    let suffix = pattern.get(suffix_start..)?;

    let min_len = prefix.len().saturating_add(suffix.len());
    if text.starts_with(prefix) && text.ends_with(suffix) && text.len() >= min_len {
        let end = text.len().saturating_sub(suffix.len());
        let extracted = text.get(prefix.len()..end)?;
        return Some(source.replace("{0}", extracted));
    }
    None
}

fn get_embedded_i18n(lang: &str) -> Option<&'static str> {
    let bytes = match lang {
        "_messages" => embedded_json::get_json(I18N_MESSAGES_ASSET, &I18N_MESSAGES_CACHE).ok()?,
        "cs" => embedded_json::get_json(I18N_CS_ASSET, &I18N_CS_CACHE).ok()?,
        "de" => embedded_json::get_json(I18N_DE_ASSET, &I18N_DE_CACHE).ok()?,
        "en" => embedded_json::get_json(I18N_EN_ASSET, &I18N_EN_CACHE).ok()?,
        "es" => embedded_json::get_json(I18N_ES_ASSET, &I18N_ES_CACHE).ok()?,
        "fr" => embedded_json::get_json(I18N_FR_ASSET, &I18N_FR_CACHE).ok()?,
        "it" => embedded_json::get_json(I18N_IT_ASSET, &I18N_IT_CACHE).ok()?,
        "ja" => embedded_json::get_json(I18N_JA_ASSET, &I18N_JA_CACHE).ok()?,
        "ko" => embedded_json::get_json(I18N_KO_ASSET, &I18N_KO_CACHE).ok()?,
        "pl" => embedded_json::get_json(I18N_PL_ASSET, &I18N_PL_CACHE).ok()?,
        "pt-BR" => embedded_json::get_json(I18N_PT_BR_ASSET, &I18N_PT_BR_CACHE).ok()?,
        "ru" => embedded_json::get_json(I18N_RU_ASSET, &I18N_RU_CACHE).ok()?,
        "tr" => embedded_json::get_json(I18N_TR_ASSET, &I18N_TR_CACHE).ok()?,
        "uk" => embedded_json::get_json(I18N_UK_ASSET, &I18N_UK_CACHE).ok()?,
        "zh-CN" => embedded_json::get_json(I18N_ZH_CN_ASSET, &I18N_ZH_CN_CACHE).ok()?,
        "zh-TW" => embedded_json::get_json(I18N_ZH_TW_ASSET, &I18N_ZH_TW_CACHE).ok()?,
        _ => return None,
    };
    std::str::from_utf8(bytes).ok()
}

/// Helper struct for terse UI localization calls.
pub struct L;

impl L {
    /// Translates a Russian source string into the active language.
    #[must_use]
    pub fn t(russian: &str, args: &[&dyn std::fmt::Display]) -> String {
        I18nService::instance().tr(russian, args)
    }
}

/// Result of translation completeness validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    /// Whether all checks passed without any errors.
    pub success: bool,
    /// Total count of master messages.
    pub total_messages: usize,
    /// Number of locales checked.
    pub checked_locales: usize,
    /// Detailed list of errors found.
    pub errors: Vec<String>,
    /// Count of translated entries per locale.
    pub translated_counts: HashMap<String, usize>,
}

/// Checker that verifies translation completeness and placeholder integrity across all locales.
pub struct I18nCompletenessChecker;

impl I18nCompletenessChecker {
    /// Validates completeness and consistency across all shipped translations.
    #[must_use]
    pub fn validate(service: Option<&I18nService>) -> ValidationResult {
        let fallback_instance;
        let svc = match service {
            Some(s) => s,
            None => {
                fallback_instance = I18nService::instance();
                fallback_instance
            }
        };
        let mut errors = Vec::new();
        let mut translated_counts = HashMap::new();

        let mut master_keys = Vec::new();
        if let Ok(en_catalog) = svc.get_catalog("en") {
            master_keys = en_catalog.keys().cloned().collect();
        }

        if master_keys.is_empty() {
            return ValidationResult {
                success: false,
                total_messages: 0,
                checked_locales: 0,
                errors: vec!["No master messages found in _messages.json or en.json".to_string()],
                translated_counts,
            };
        }

        let required_languages = &["en", "uk"];
        for &lang in required_languages {
            if let Ok(catalog) = svc.get_catalog(lang) {
                let missing: Vec<&String> = master_keys
                    .iter()
                    .filter(|key| {
                        !catalog.contains_key(*key) || catalog.get(*key).is_some_and(|v| matches!(v, JsonValue::Null))
                    })
                    .collect();
                if !missing.is_empty() {
                    errors.push(format!(
                        "Language '{lang}' is missing {} required translations. First missing: '{}'",
                        missing.len(),
                        missing.first().map(|s| s.as_str()).unwrap_or("")
                    ));
                }
            } else {
                errors.push(format!("Could not load catalog for required language '{lang}'."));
            }
        }

        let all_languages: Vec<&str> = SUPPORTED_LANGUAGES
            .iter()
            .map(|(code, _)| *code)
            .filter(|&code| code != "ru")
            .collect();

        for &lang in &all_languages {
            if let Ok(catalog) = svc.get_catalog(lang) {
                let mut valid_count = 0usize;
                for key in &master_keys {
                    if let Some(element) = catalog.get(key) {
                        valid_count = valid_count.saturating_add(1);
                        if let Some(translated) = element.as_str() {
                            let src_placeholders = extract_placeholders(key);
                            let dst_placeholders = extract_placeholders(translated);
                            if src_placeholders != dst_placeholders {
                                errors.push(format!(
                                    "Placeholder mismatch in '{lang}' for key '{key}': source={src_placeholders:?}, target={dst_placeholders:?}"
                                ));
                            }
                        }
                    }
                }
                translated_counts.insert(lang.to_string(), valid_count);
            }
        }

        validate_plural_rules(svc, &mut errors);
        validate_fallback_chain(svc, &mut errors);

        ValidationResult {
            success: errors.is_empty(),
            total_messages: master_keys.len(),
            checked_locales: all_languages.len(),
            errors,
            translated_counts,
        }
    }
}

fn extract_placeholders(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes.get(i) == Some(&b'{') {
            let start = i.saturating_add(1);
            let mut j = start;
            while j < bytes.len()
                && bytes.get(j) != Some(&b'}')
                && bytes.get(j) != Some(&b':')
                && bytes.get(j) != Some(&b'!')
            {
                j = j.saturating_add(1);
            }
            if let Some(slice) = text.get(start..j) {
                if let Ok(n) = slice.parse::<u32>() {
                    result.push(n.to_string());
                }
            }
            while j < bytes.len() && bytes.get(j) != Some(&b'}') {
                j = j.saturating_add(1);
            }
            i = j;
        }
        i = i.saturating_add(1);
    }
    result.sort();
    result
}

fn validate_plural_rules(service: &I18nService, errors: &mut Vec<String>) {
    let test_cases = &[
        ("ru", 1, "сохранение"),
        ("ru", 3, "сохранения"),
        ("ru", 11, "сохранений"),
        ("uk", 22, "збереження"),
        ("uk", 25, "збережень"),
        ("en", 1, "save"),
        ("en", 2, "saves"),
        ("pl", 1, "zapis"),
        ("pl", 3, "zapisy"),
        ("pl", 5, "zapisów"),
        ("fr", 0, "sauvegarde"),
        ("ja", 7, "件のセーブ"),
    ];

    for &(lang, count, expected) in test_cases {
        let actual = service.trn_in(Some(lang), count, "сохранение", "сохранения", "сохранений", &[]);
        if actual != expected {
            errors.push(format!(
                "Plural rule failure for '{lang}' with count {count}: expected '{expected}', actual '{actual}'"
            ));
        }
    }
}

fn validate_fallback_chain(service: &I18nService, errors: &mut Vec<String>) {
    let ru_text = service.tr_in(Some("ru"), "Сохранить", &[]);
    if ru_text != "Сохранить" {
        errors.push(format!("Ru source failed: expected 'Сохранить', got '{ru_text}'"));
    }

    let en_text = service.tr_in(Some("en"), "Сохранить", &[]);
    if en_text != "Save" {
        errors.push(format!("En translation failed: expected 'Save', got '{en_text}'"));
    }

    let non_existent = service.tr_in(Some("de"), "НесуществующийКлюч123", &[]);
    if non_existent != "НесуществующийКлюч123" {
        errors.push(format!(
            "Fallback for non-existent key failed: expected original text, got '{non_existent}'"
        ));
    }

    let de_text = service.tr_in(
        Some("de"),
        "Запись в облако недоступна: {0}; запись не начиналась",
        &[&"offline"],
    );
    service.set_language("de");
    let recovered = service.source_text(&de_text);
    if recovered != "Запись в облако недоступна: offline; запись не начиналась"
    {
        errors.push(format!(
            "SourceText recovery failed: expected 'Запись в облако недоступна: offline; запись не начиналась', got '{recovered}'"
        ));
    }
    service.set_language("ru");
}
