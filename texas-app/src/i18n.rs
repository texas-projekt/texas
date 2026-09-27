/// Application-level internationalization for user-visible UI text.
///
/// `I18n` deliberately lives in `CommonData` instead of Floem's global
/// context. Floem's context store is runtime-global in the pinned version,
/// while Texas can create multiple window tabs with different workspace
/// configuration. A window-local signal keeps those scopes independent.
use std::{collections::HashMap, env};

use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};
use once_cell::sync::Lazy;
const EN_LOCALE: &str = include_str!("../assets/locales/en.toml");
const ZH_CN_LOCALE: &str = include_str!("../assets/locales/zh-CN.toml");

static EN: Lazy<HashMap<String, String>> = Lazy::new(|| parse_locale(EN_LOCALE));
static ZH_CN: Lazy<HashMap<String, String>> =
    Lazy::new(|| parse_locale(ZH_CN_LOCALE));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    En,
    ZhCn,
}

impl Locale {
    fn from_tag(tag: &str) -> Option<Self> {
        let tag = tag.trim().to_ascii_lowercase();
        if tag == "en" || tag.starts_with("en-") || tag.starts_with("en_") {
            Some(Self::En)
        } else if tag == "zh"
            || tag == "zh-cn"
            || tag == "zh_cn"
            || tag == "zh-hans"
            || tag.starts_with("zh-hans-")
            || tag.starts_with("zh-cn.")
            || tag.starts_with("zh_cn.")
        {
            Some(Self::ZhCn)
        } else {
            None
        }
    }

    fn detect_system() -> Self {
        #[cfg(windows)]
        if let Some(locale) =
            sys_locale::get_locales().find_map(|tag| Self::from_tag(&tag))
        {
            return locale;
        }

        ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"]
            .into_iter()
            .filter_map(|key| env::var(key).ok())
            .find_map(|value| Self::from_tag(&value))
            .unwrap_or(Self::En)
    }

    pub fn from_preference(preference: &str) -> Self {
        if preference.eq_ignore_ascii_case("auto") {
            Self::detect_system()
        } else {
            Self::from_tag(preference).unwrap_or(Self::En)
        }
    }
}

#[derive(Clone)]
pub struct I18n {
    locale: RwSignal<Locale>,
}

impl I18n {
    pub fn new(cx: Scope, preference: &str) -> Self {
        Self {
            locale: cx.create_rw_signal(Locale::from_preference(preference)),
        }
    }

    pub fn locale(&self) -> Locale {
        self.locale.get()
    }

    pub fn set_preference(&self, preference: &str) {
        self.locale.set(Locale::from_preference(preference));
    }

    pub fn text(&self, key: &str) -> String {
        self.template(key).unwrap_or_else(|| key.to_owned())
    }

    /// Translates `key`, substituting `{name}` placeholders with the given
    /// arguments.
    pub fn text_with_args(
        &self,
        key: &str,
        fallback: &str,
        args: &[(&str, &str)],
    ) -> String {
        match self.template(key) {
            Some(template) => substitute(&template, args),
            None => substitute(fallback, args),
        }
    }

    fn template(&self, key: &str) -> Option<String> {
        let translations = match self.locale.get() {
            Locale::En => &*EN,
            Locale::ZhCn => &*ZH_CN,
        };

        translations.get(key).or_else(|| EN.get(key)).cloned()
    }

    /// Translate a command description while preserving the built-in English
    /// description as a fallback for commands that do not yet have a locale
    /// entry.
    pub fn command_text(&self, command_id: &str, fallback: &str) -> String {
        let key = format!("command.{command_id}");
        let translated = self.text(&key);
        if translated == key {
            fallback.to_owned()
        } else {
            translated
        }
    }

    pub fn setting_text(
        &self,
        kind: &str,
        field: &str,
        part: &str,
        fallback: &str,
    ) -> String {
        let key = format!(
            "settings.item.{}.{}.{}",
            kind.to_ascii_lowercase(),
            field,
            part
        );
        let translated = self.text(&key);
        if translated == key {
            fallback.to_owned()
        } else {
            translated
        }
    }

    pub fn setting_value_text(&self, field: &str, value: &str) -> String {
        let key = format!("settings.value.{field}.{value}");
        let translated = self.text(&key);
        if translated == key {
            value.to_owned()
        } else {
            translated
        }
    }

    /// Creates a reactive text producer for Floem views.
    ///
    /// The locale signal is read when the returned closure runs, so views such
    /// as `label(i18n.text_signal("panel.file-explorer"))` update when the
    /// language changes. Keeping this separate from [`Self::text`] makes it
    /// harder to accidentally snapshot a translation during view construction.
    pub fn text_signal(
        &self,
        key: &'static str,
    ) -> impl Fn() -> String + Clone + 'static + use<> {
        let i18n = self.clone();
        move || i18n.text(key)
    }
}

fn parse_locale(source: &str) -> HashMap<String, String> {
    toml::from_str(source).expect("embedded locale file must be valid TOML")
}

/// Translates `key` without a reactive [`I18n`], for contexts such as native
/// dialogs that are shown before the application state exists. Uses the
/// system locale.
pub fn system_text(key: &str) -> String {
    static SYSTEM_LOCALE: Lazy<Locale> = Lazy::new(Locale::detect_system);

    let translations = match *SYSTEM_LOCALE {
        Locale::En => &*EN,
        Locale::ZhCn => &*ZH_CN,
    };

    translations
        .get(key)
        .or_else(|| EN.get(key))
        .cloned()
        .unwrap_or_else(|| key.to_owned())
}

fn substitute(template: &str, args: &[(&str, &str)]) -> String {
    let mut result = template.to_owned();
    for (name, value) in args {
        result = result.replace(&format!("{{{name}}}"), value);
    }
    result
}

#[cfg(test)]
fn placeholders(template: &str) -> std::collections::BTreeSet<&str> {
    let mut result = std::collections::BTreeSet::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('}') else {
            break;
        };
        let name = &after_start[..end];
        if !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            result.insert(name);
        }
        rest = &after_start[end + 1..];
    }
    result
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    use regex::Regex;

    use crate::config::{
        core::CoreConfig, editor::EditorConfig, terminal::TerminalConfig,
        ui::UIConfig,
    };

    use super::*;

    #[test]
    fn locale_files_have_matching_keys() {
        assert_eq!(EN.len(), ZH_CN.len());
        assert!(EN.keys().all(|key| ZH_CN.contains_key(key)));
        for key in EN.keys() {
            assert_eq!(
                placeholders(&EN[key]),
                placeholders(&ZH_CN[key]),
                "placeholder mismatch: {key}"
            );
        }
    }

    #[test]
    fn locale_tags_are_normalized_conservatively() {
        assert_eq!(Locale::from_preference("en-US"), Locale::En);
        assert_eq!(Locale::from_preference("zh-CN"), Locale::ZhCn);
        assert_eq!(Locale::from_tag("zh-Hans-CN"), Some(Locale::ZhCn));
        assert_ne!(Locale::from_tag("zh-TW"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_preference("fr"), Locale::En);
    }

    #[test]
    fn all_commands_have_locale_entries() {
        for command in crate::command::texas_internal_commands().values() {
            let key = format!("command.{}", command.kind.str());
            assert!(EN.contains_key(&key), "missing English command key: {key}");
            assert!(
                ZH_CN.contains_key(&key),
                "missing Simplified Chinese command key: {key}"
            );
        }
    }

    #[test]
    fn all_settings_have_locale_entries() {
        let mut problems = Vec::new();
        for (kind, fields) in [
            ("core", &CoreConfig::FIELDS[..]),
            ("editor", &EditorConfig::FIELDS[..]),
            ("ui", &UIConfig::FIELDS[..]),
            ("terminal", &TerminalConfig::FIELDS[..]),
        ] {
            for field in fields {
                let field = field.replace('_', "-");
                for part in ["name", "description"] {
                    let key = format!("settings.item.{kind}.{field}.{part}");
                    if !EN.contains_key(&key) {
                        problems
                            .push(format!("missing English settings key: {key}"));
                    }
                    if !ZH_CN.contains_key(&key) {
                        problems.push(format!(
                            "missing Simplified Chinese settings key: {key}"
                        ));
                    } else if !contains_cjk(&ZH_CN[&key]) {
                        problems.push(format!(
                            "Simplified Chinese settings value contains no Chinese text: {key}"
                        ));
                    }
                }
            }
        }
        assert!(
            problems.is_empty(),
            "settings translation problems:\n{}",
            problems.join("\n")
        );
    }

    #[test]
    fn referenced_keys_exist_in_english_locale() {
        let mut files = Vec::new();
        collect_rust_files(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );

        let mut missing = Vec::new();
        for file in files {
            let text = fs::read_to_string(&file).unwrap();
            let code = rust_code_mask(&text);
            for captures in translation_key_pattern().captures_iter(&text) {
                let start = captures.get(0).unwrap().start();
                if !code[start] {
                    continue;
                }
                let key = captures[1].to_string();
                if !EN.contains_key(&key) {
                    let line =
                        text[..start].bytes().filter(|byte| *byte == b'\n').count()
                            + 1;
                    missing.push(format!("{}:{}: {key}", file.display(), line));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "referenced translation keys missing from the English locale:\n{}",
            missing.join("\n")
        );
    }

    fn translation_key_pattern() -> &'static Regex {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        PATTERN.get_or_init(|| {
            Regex::new(r#"\b(?:text|text_signal)\(\s*"([A-Za-z0-9_.-]+)""#).unwrap()
        })
    }

    fn rust_code_mask(source: &str) -> Vec<bool> {
        let bytes = source.as_bytes();
        let mut code = vec![true; bytes.len()];
        let mut index = 0;

        while index < bytes.len() {
            if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
                let start = index;
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
                mask_non_code(&mut code, start, index);
            } else if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                let start = index;
                index += 2;
                let mut depth = 1;
                while index + 1 < bytes.len() && depth > 0 {
                    if bytes[index] == b'/' && bytes[index + 1] == b'*' {
                        depth += 1;
                        index += 2;
                    } else if bytes[index] == b'*' && bytes[index + 1] == b'/' {
                        depth -= 1;
                        index += 2;
                    } else {
                        index += 1;
                    }
                }
                if depth > 0 {
                    index = bytes.len();
                }
                mask_non_code(&mut code, start, index.min(bytes.len()));
            } else if bytes[index] == b'r' {
                if let Some(end) = raw_string_end(bytes, index) {
                    mask_non_code(&mut code, index, end);
                    index = end;
                } else {
                    index += 1;
                }
            } else if bytes[index] == b'\'' {
                if let Some(end) = char_literal_end(bytes, index) {
                    mask_non_code(&mut code, index, end);
                    index = end;
                } else {
                    index += 1;
                }
            } else if bytes[index] == b'"' {
                let end = skip_quoted(bytes, index);
                mask_non_code(&mut code, index, end);
                index = end;
            } else {
                index += 1;
            }
        }

        code
    }

    fn mask_non_code(code: &mut [bool], start: usize, end: usize) {
        for position in &mut code[start..end] {
            *position = false;
        }
    }

    fn skip_quoted(source: &[u8], start: usize) -> usize {
        let mut index = start + 1;
        while index < source.len() {
            if source[index] == b'\\' {
                index += 2;
            } else if source[index] == b'"' {
                return index + 1;
            } else if source[index] == b'\n' {
                return start + 1;
            } else {
                index += 1;
            }
        }
        source.len()
    }

    fn char_literal_end(source: &[u8], start: usize) -> Option<usize> {
        let mut index = start + 1;
        if source.get(index) == Some(&b'\\') {
            index += 2;
        } else {
            index += 1;
        }
        if source.get(index) == Some(&b'\'') {
            Some(index + 1)
        } else {
            None
        }
    }

    fn raw_string_end(source: &[u8], start: usize) -> Option<usize> {
        let mut hashes = 0;
        let mut index = start + 1;
        while source.get(index) == Some(&b'#') {
            hashes += 1;
            index += 1;
        }
        if source.get(index) != Some(&b'"') {
            return None;
        }

        index += 1;
        while index < source.len() {
            if source[index] == b'"'
                && source
                    .get(index + 1..index + 1 + hashes)
                    .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'))
            {
                return Some(index + 1 + hashes);
            }
            index += 1;
        }
        Some(source.len())
    }

    #[test]
    fn text_with_args_substitutes_named_placeholders() {
        let i18n = I18n::new(Scope::new(), "en");
        let text = i18n.text_with_args(
            "terminal.launch-error",
            "Terminal failed to launch. Error: {error}",
            &[("error", "boom")],
        );
        assert_eq!(text, "Terminal failed to launch. Error: boom");
    }

    #[test]
    fn system_text_returns_key_when_missing() {
        let missing = format!("i18n.{}", "does-not-exist");
        assert_eq!(system_text(&missing), missing);
    }

    #[test]
    fn substitute_replaces_every_occurrence() {
        assert_eq!(substitute("a {x} b {x}", &[("x", "1")]), "a 1 b 1");
    }

    #[test]
    fn comments_are_not_scanned_for_translation_keys() {
        let source = r##"
            // i18n.text("commented-out")
            /* i18n.text_signal("block-comment") /* nested */ */
            let quote = '"';
            i18n.text("real-key");
        "##;
        let code = rust_code_mask(source);
        let keys = translation_key_pattern()
            .captures_iter(source)
            .filter(|captures| code[captures.get(0).unwrap().start()])
            .map(|captures| captures[1].to_string())
            .collect::<Vec<_>>();
        assert_eq!(keys, ["real-key"]);
    }

    #[test]
    fn missing_key_line_uses_call_start() {
        let source = "let value = 1;\ni18n.text(\"missing-key\");\n";
        let capture = translation_key_pattern().captures(source).unwrap();
        let line = source[..capture.get(0).unwrap().start()]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        assert_eq!(line, 2);
    }

    fn contains_cjk(value: &str) -> bool {
        value
            .chars()
            .any(|c| ('\u{3400}'..='\u{9fff}').contains(&c))
    }

    fn collect_rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_rust_files(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
}
