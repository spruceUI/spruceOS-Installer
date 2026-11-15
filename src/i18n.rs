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
// WHICH LANGUAGE: INSTALLER_LANG=<code> if set (handy for checking a
// translation), else the one picked in the installer's language menu, else
// the system's preferred languages, else English. The menu's choice is saved
// in the user's config folder; "System language" deletes it.
//
// SOURCE TABS: their text stays in config.rs, in English, so forks keep
// editing it there. en.json repeats it under repo.<url>.name / .info /
// .display_name, and a language translates those keys. A translation is shown
// only while en.json's English still matches config.rs exactly, so rewriting
// a tab in config.rs shows the new English instead of a stale translation.
//
// The debug log stays in English on purpose: it is what people paste into bug
// reports, so it has to be readable by whoever maintains the installer.
// ============================================================================

use crate::config::{RepoOption, TEMP_PREFIX};
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};

/// Overrides the system language when set to the code of a bundled language.
pub const LANG_ENV_VAR: &str = "INSTALLER_LANG";

/// False while a language picked in the menu is in use.
static FOLLOWS_SYSTEM: AtomicBool = AtomicBool::new(true);

/// Picks the UI language. Call once at startup, before drawing anything.
pub fn init() {
    let available = rust_i18n::available_locales!();
    let saved = saved_language().filter(|code| available.iter().any(|c| c == code));
    FOLLOWS_SYSTEM.store(saved.is_none(), Ordering::Relaxed);
    let override_lang = std::env::var(LANG_ENV_VAR).ok();
    let preferred = override_lang
        .into_iter()
        .chain(saved)
        .chain(sys_locale::get_locales());
    let locale = pick_locale(preferred, &available);
    rust_i18n::set_locale(&locale);
    crate::debug::log(&format!("UI language: {}", locale));
}

/// Codes of the bundled languages, e.g. ["ca", "en"].
pub fn languages() -> Vec<String> {
    rust_i18n::available_locales!()
        .into_iter()
        .map(|code| code.into_owned())
        .collect()
}

/// The language in use now.
pub fn current() -> String {
    rust_i18n::locale().to_string()
}

/// Whether the language comes from the system rather than the menu.
pub fn follows_system() -> bool {
    FOLLOWS_SYSTEM.load(Ordering::Relaxed)
}

/// Switches the UI language now and remembers it for the next launch.
/// None goes back to the system language and forgets the choice.
pub fn set_language(code: Option<&str>) {
    let locale = match code {
        Some(code) => code.to_string(),
        None => pick_locale(sys_locale::get_locales(), &languages()),
    };
    rust_i18n::set_locale(&locale);
    FOLLOWS_SYSTEM.store(code.is_none(), Ordering::Relaxed);
    save_language(code);
    crate::debug::log(&format!(
        "UI language: {}{}",
        locale,
        if code.is_none() { " (system)" } else { "" }
    ));
}

fn language_file() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|dir| dir.join(TEMP_PREFIX).join("language"))
}

fn saved_language() -> Option<String> {
    let text = std::fs::read_to_string(language_file()?).ok()?;
    let code = text.trim();
    (!code.is_empty()).then(|| code.to_string())
}

/// Best effort: if the folder cannot be written, the choice lasts this run.
fn save_language(code: Option<&str>) {
    let Some(path) = language_file() else { return };
    let result = match code {
        Some(code) => path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&path, code)),
        None => match std::fs::remove_file(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        },
    };
    if let Err(e) = result {
        crate::debug::log(&format!("Could not save the language choice to {:?}: {}", path, e));
    }
}

/// A source tab's name in the UI language.
pub fn repo_name(repo: &RepoOption) -> Cow<'static, str> {
    config_text(&repo_key(repo, "name"), repo.name)
}

/// A source tab's info text in the UI language.
pub fn repo_info(repo: &RepoOption) -> Cow<'static, str> {
    config_text(&repo_key(repo, "info"), repo.info)
}

/// The name the success and error dialogs use: display_name, else the tab name.
pub fn repo_display_name(repo: &RepoOption) -> Cow<'static, str> {
    match repo.display_name {
        Some(display_name) => config_text(&repo_key(repo, "display_name"), display_name),
        None => repo_name(repo),
    }
}

fn repo_key(repo: &RepoOption, field: &str) -> String {
    format!("repo.{}.{}", repo.url, field)
}

/// config.rs text in the UI language. Translated only while en.json holds the
/// same English under `key`; otherwise the config.rs English as written.
fn config_text(key: &str, english: &'static str) -> Cow<'static, str> {
    if t!(key, locale = "en") == english {
        Cow::Owned(t!(key).into_owned())
    } else {
        Cow::Borrowed(english)
    }
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

    fn links(text: &str) -> BTreeSet<&str> {
        text.match_indices("](")
            .filter_map(|(start, _)| {
                let rest = &text[start + 2..];
                rest.find(')').map(|end| &rest[..end])
            })
            .collect()
    }

    #[test]
    fn translations_keep_link_targets() {
        let all = locales();
        for (code, strings) in &all {
            for (key, english) in &all[REFERENCE] {
                let Some(text) = strings.get(key) else { continue };
                assert_eq!(links(text), links(english), "{}.json \"{}\" changes a [link](url)", code, key);
            }
        }
    }

    #[test]
    fn en_json_repeats_the_tabs_in_config_rs() {
        // Without this, editing a tab in config.rs and forgetting en.json would
        // quietly show that tab in English in every language.
        let en = &locales()[REFERENCE];
        for repo in crate::config::REPO_OPTIONS {
            let mut fields = vec![("name", repo.name), ("info", repo.info)];
            fields.extend(repo.display_name.map(|d| ("display_name", d)));
            for (field, text) in fields {
                let key = repo_key(repo, field);
                assert_eq!(
                    en.get(&key).map(String::as_str),
                    Some(text),
                    "en.json \"{}\" must match config.rs (copy the new English there, or remove the key)",
                    key
                );
            }
        }
        for key in en.keys().filter(|k| k.starts_with("repo.")) {
            let known = crate::config::REPO_OPTIONS.iter().any(|repo| {
                ["name", "info", "display_name"].iter().any(|f| repo_key(repo, f) == *key)
            });
            assert!(known, "en.json \"{}\" matches no tab in config.rs", key);
        }
    }

    #[test]
    fn tabs_are_translated_and_fall_back_to_config_rs() {
        let stable = &crate::config::REPO_OPTIONS[0];
        rust_i18n::set_locale("ca");
        assert_ne!(repo_info(stable), stable.info, "Stable's info should be in Catalan");
        assert_eq!(config_text("repo.example/fork.name", "My Fork"), "My Fork");
        // A tab whose English no longer matches en.json stays in English
        assert_eq!(config_text(&repo_key(stable, "name"), "Stable builds"), "Stable builds");
        rust_i18n::set_locale("en");
        assert_eq!(repo_info(stable), stable.info);
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
        assert_eq!(t!("common.cancel", locale = "xx"), "Cancel");
        assert_eq!(
            t!("complete.message", locale = "ca", name = "spruceOS Stable"),
            "S'ha instal·lat spruceOS Stable correctament."
        );
    }
}
