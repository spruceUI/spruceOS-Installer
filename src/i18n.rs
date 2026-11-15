// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

// ============================================================================
// TRANSLATIONS
// ============================================================================
// UI text lives in locales/<language>.json (en.json is the reference) and is
// looked up with rust_i18n's t!("key") or t!("key", name = value) for
// %{name} placeholders. A key missing from a language falls back to English.
//
// ADDING A LANGUAGE:
//   1. Copy locales/en.json to locales/<code>.json, where <code> is the
//      two-letter ISO 639-1 code (ca, es, fr...), and translate the values.
//   2. Keep every %{placeholder} and every \n line break. The tests below
//      check both, and that no key is missing or left over.
//
// The language follows the system's preferred languages. INSTALLER_LANG=<code>
// overrides it, which is handy for checking a translation.
//
// The debug log stays in English on purpose: it is what people paste into bug
// reports, so it has to be readable by whoever maintains the installer.
// ============================================================================

/// Overrides the system language when set to the code of a bundled language.
pub const LANG_ENV_VAR: &str = "INSTALLER_LANG";

/// Picks the UI language. Call once at startup, before drawing anything.
pub fn init() {
    let available = rust_i18n::available_locales!();
    let override_lang = std::env::var(LANG_ENV_VAR).ok();
    let preferred = override_lang.into_iter().chain(sys_locale::get_locales());
    let locale = pick_locale(preferred, &available);
    rust_i18n::set_locale(&locale);
    crate::debug::log(&format!("UI language: {}", locale));
}

/// First preferred language that has a translation, by its language part
/// ("ca-ES", "ca_ES.UTF-8" and "ca-ES-valencia" are all "ca"), else English.
fn pick_locale<I, S, A>(preferred: I, available: &[A]) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    A: AsRef<str>,
{
    preferred
        .into_iter()
        .map(|tag| language_part(tag.as_ref()))
        .find(|lang| available.iter().any(|code| code.as_ref() == lang))
        .unwrap_or_else(|| "en".to_string())
}

fn language_part(tag: &str) -> String {
    tag.split(['-', '_', '.', '@'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    const REFERENCE: &str = "en";

    fn load(path: &std::path::Path) -> BTreeMap<String, String> {
        let text = std::fs::read_to_string(path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} is not valid JSON: {}", path.display(), e));
        value
            .as_object()
            .unwrap_or_else(|| panic!("{} must be a JSON object", path.display()))
            .iter()
            .filter(|(key, _)| !key.starts_with('_'))
            .map(|(key, value)| {
                let text = value
                    .as_str()
                    .unwrap_or_else(|| panic!("{}: \"{}\" must be a string", path.display(), key));
                (key.clone(), text.to_string())
            })
            .collect()
    }

    fn locales() -> BTreeMap<String, BTreeMap<String, String>> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .map(|path| {
                let code = path.file_stem().unwrap().to_string_lossy().into_owned();
                (code, load(&path))
            })
            .collect()
    }

    fn placeholders(text: &str) -> BTreeSet<&str> {
        text.match_indices("%{")
            .filter_map(|(start, _)| {
                let rest = &text[start + 2..];
                rest.find('}').map(|end| &rest[..end])
            })
            .collect()
    }

    #[test]
    fn every_language_has_exactly_the_reference_keys() {
        let all = locales();
        let reference: BTreeSet<_> = all[REFERENCE].keys().collect();
        for (code, strings) in &all {
            let keys: BTreeSet<_> = strings.keys().collect();
            let missing: Vec<_> = reference.difference(&keys).collect();
            let extra: Vec<_> = keys.difference(&reference).collect();
            assert!(missing.is_empty(), "{}.json is missing {:?}", code, missing);
            assert!(extra.is_empty(), "{}.json has keys en.json lacks: {:?}", code, extra);
        }
    }

    #[test]
    fn translations_keep_placeholders_and_line_breaks() {
        let all = locales();
        for (code, strings) in &all {
            for (key, english) in &all[REFERENCE] {
                let Some(text) = strings.get(key) else { continue };
                assert_eq!(
                    placeholders(text),
                    placeholders(english),
                    "{}.json \"{}\" changes the %{{placeholders}}",
                    code,
                    key
                );
                assert_eq!(
                    text.matches('\n').count(),
                    english.matches('\n').count(),
                    "{}.json \"{}\" changes the number of line breaks",
                    code,
                    key
                );
                assert!(!text.trim().is_empty(), "{}.json \"{}\" is empty", code, key);
            }
        }
    }

    #[test]
    fn every_bundled_language_is_compiled_in() {
        let compiled = rust_i18n::available_locales!();
        for code in locales().keys() {
            assert!(compiled.iter().any(|c| c == code), "{} is not compiled in", code);
        }
    }

    #[test]
    fn picks_first_preferred_language_with_a_translation() {
        let available = ["ca", "en"];
        assert_eq!(pick_locale(["ca-ES"], &available), "ca");
        assert_eq!(pick_locale(["ca_ES.UTF-8"], &available), "ca");
        assert_eq!(pick_locale(["ca-ES-valencia"], &available), "ca");
        assert_eq!(pick_locale(["CA"], &available), "ca");
        assert_eq!(pick_locale(["fr-FR", "ca-ES", "en-US"], &available), "ca");
        assert_eq!(pick_locale(["en-GB", "ca-ES"], &available), "en");
        assert_eq!(pick_locale(["fr-FR", "de-DE"], &available), "en");
        assert_eq!(pick_locale(Vec::<String>::new(), &available), "en");
    }

    #[test]
    fn lookups_use_the_chosen_language_and_fall_back_to_english() {
        assert_eq!(t!("common.cancel", locale = "en"), "Cancel");
        assert_eq!(t!("common.cancel", locale = "ca"), "Cancel·la");
        assert_eq!(t!("common.cancel", locale = "ja"), "Cancel");
        assert_eq!(
            t!("complete.message", locale = "ca", name = "spruceOS Stable"),
            "S'ha instal·lat spruceOS Stable correctament."
        );
    }
}
