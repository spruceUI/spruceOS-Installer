// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

use std::collections::HashMap;
use std::sync::OnceLock;

/// Embedded MAME names XML file
const MAME_NAMES_XML: &str = include_str!("../assets/boxartdb/mame_names.xml");

/// Get the MAME ROM code to display name mapping
/// Parsed lazily on first access and cached
pub fn get_mame_mapping() -> &'static HashMap<String, String> {
    static MAME_MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    MAME_MAP.get_or_init(|| parse_mame_xml(MAME_NAMES_XML))
}

/// Parse the MAME XML file and build a HashMap of ROM codes to display names
fn parse_mame_xml(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();

    // Simple XML parsing - extract path and name from each <game> entry
    let mut current_path: Option<String> = None;

    for line in xml.lines() {
        let trimmed = line.trim();

        // Extract ROM code from <path>./romcode.zip</path>
        if let Some(path_start) = trimmed.strip_prefix("<path>./") {
            if let Some(path_end) = path_start.strip_suffix("</path>") {
                // Remove .zip extension if present
                let rom_code = path_end
                    .strip_suffix(".zip")
                    .unwrap_or(path_end)
                    .to_string();
                current_path = Some(rom_code);
            }
        }

        // Extract display name from <name>Display Name</name>
        if let Some(name_start) = trimmed.strip_prefix("<name>") {
            if let Some(name_end) = name_start.strip_suffix("</name>") {
                if let Some(rom_code) = current_path.take() {
                    map.insert(rom_code, name_end.to_string());
                }
            }
        }
    }

    crate::debug::log(&format!("MAME database: Loaded {} ROM mappings", map.len()));
    map
}

/// Get the display name for a MAME ROM code
/// Returns None if not found in the database
pub fn get_display_name(rom_code: &str) -> Option<&'static str> {
    let map = get_mame_mapping();
    map.get(rom_code).map(|s| s.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mame_xml() {
        let map = get_mame_mapping();

        // Should have loaded entries
        assert!(!map.is_empty());

        // Test known entries from the XML
        assert_eq!(map.get("gtmr"), Some(&"1000 Miglia - Great 1000 Miles Rally".to_string()));
        assert_eq!(map.get("1941"), Some(&"1941: Counter Attack".to_string()));
        assert_eq!(map.get("1942"), Some(&"1942".to_string()));
    }

    #[test]
    fn test_get_display_name() {
        assert_eq!(get_display_name("gtmr"), Some("1000 Miglia - Great 1000 Miles Rally"));
        assert_eq!(get_display_name("nonexistent"), None);
    }
}
