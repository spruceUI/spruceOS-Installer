// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::sync::mpsc;
use tokio::fs;
use regex::Regex;
use image::ImageFormat;

use crate::boxart_db;
use crate::mame_db;

/// Statistics for a boxart scraping operation
#[derive(Debug, Clone, Default)]
pub struct ScrapeStats {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
}

/// Progress updates during scraping
#[derive(Debug, Clone)]
pub enum ScrapeProgress {
    Started { total: usize },
    Progress { current: usize, rom_name: String, status: String },
    Completed(ScrapeStats),
}

/// Main boxart scraper struct
pub struct BoxArtScraper {
    cache: HashMap<String, Vec<(String, HashSet<String>)>>,
    preferred_region: Option<String>,
}

impl BoxArtScraper {
    /// Create a new BoxArtScraper with default settings from config
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            preferred_region: crate::config::SCRAPER_CONFIG.preferred_region.map(|s| s.to_string()),
        }
    }

    /// Get the system mapping for a folder name
    fn get_system_mapping(folder_name: &str) -> Option<&'static crate::config::SystemMapping> {
        crate::config::SYSTEM_MAPPINGS
            .iter()
            .find(|m| m.folder_name.eq_ignore_ascii_case(folder_name))
    }

    /// Get the Libretro alias name for a system
    pub fn get_ra_alias(sys_name: &str) -> Option<&'static str> {
        Self::get_system_mapping(sys_name).map(|m| m.libretro_name)
    }

    /// Check if a system uses MAME-style ROM naming (requires XML lookup)
    fn is_arcade_system(sys_name: &str) -> bool {
        matches!(
            sys_name.to_uppercase().as_str(),
            "ARCADE" | "NEOGEO" | "CPS1" | "CPS2" | "CPS3" | "FBNEO" | "MAME2003PLUS"
        )
    }

    /// Find the best matching image name for a given ROM
    pub fn find_image_name(&mut self, sys_name: &str, rom_name: &str) -> Option<String> {
        // Load and cache the image list for this system from embedded database
        if !self.cache.contains_key(sys_name) {
            let content = boxart_db::get_boxart_db(sys_name)?;

            let image_list: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let tokenized: Vec<(String, HashSet<String>)> = image_list
                .iter()
                .map(|name| {
                    let stripped = Self::normalize_name(&name.replace(".png", ""));
                    (name.clone(), Self::tokenize(&stripped))
                })
                .collect();
            self.cache.insert(sys_name.to_string(), tokenized);
        }

        let rom_without_ext = Path::new(rom_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(rom_name);

        // For arcade systems, look up the display name in MAME database first
        // If not found, skip this ROM entirely (don't attempt fallback matching)
        let search_name = if Self::is_arcade_system(sys_name) {
            match mame_db::get_display_name(rom_without_ext) {
                Some(display_name) => {
                    crate::debug::log(&format!(
                        "MAME lookup: {} -> {}",
                        rom_without_ext,
                        display_name
                    ));
                    display_name
                }
                None => {
                    crate::debug::log(&format!(
                        "MAME lookup: {} not found in database, skipping",
                        rom_without_ext
                    ));
                    return None; // Skip ROMs not in MAME database
                }
            }
        } else {
            rom_without_ext
        };

        self.find_image_from_list(sys_name, search_name)
    }

    fn re_parens() -> &'static Regex {
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new(r"\([^)]*\)").unwrap())
    }

    fn re_brackets() -> &'static Regex {
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new(r"\[[^\]]*\]").unwrap())
    }

    fn re_spaces() -> &'static Regex {
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new(r"[\s\-_,]+").unwrap())
    }

    fn re_punct() -> &'static Regex {
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new(r"[^\w\s]+").unwrap())
    }

    fn re_capture_parens() -> &'static Regex {
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new(r"\(([^)]*?)\)").unwrap())
    }

    /// Strip parentheses, brackets, and normalize spaces/symbols
    fn normalize_name(s: &str) -> String {
        let without_parens = Self::re_parens().replace_all(s, "");
        let without_brackets = Self::re_brackets().replace_all(&without_parens, "");
        Self::re_spaces().replace_all(&without_brackets, " ").trim().to_string()
    }

    /// Preprocess a single token (expand abbreviations, convert numbers to roman)
    fn preprocess_token(token: &str) -> String {
        let token_lower = token.to_lowercase();

        // Abbreviation expansion
        let abbreviations: HashMap<&str, &str> = [
            ("ff", "final fantasy"),
            ("zelda", "legend of zelda"),
            ("mario", "super mario"),
            ("smb", "super mario bros"),
            ("smw", "super mario world"),
            ("sf", "street fighter"),
            ("mk", "mortal kombat"),
            ("dkc", "donkey kong country"),
            ("cv", "castlevania"),
            ("mm", "mega man"),
            ("dr", "doctor"),
            ("st", "saint"),
            ("mr", "mister"),
        ]
        .iter()
        .cloned()
        .collect();

        if let Some(expansion) = abbreviations.get(token_lower.as_str()) {
            return expansion.to_string();
        }

        // Number to roman numeral conversion
        let num_to_roman: HashMap<&str, &str> = [
            ("1", "i"), ("2", "ii"), ("3", "iii"), ("4", "iv"), ("5", "v"),
            ("6", "vi"), ("7", "vii"), ("8", "viii"), ("9", "ix"), ("10", "x"),
        ]
        .iter()
        .cloned()
        .collect();

        if let Some(roman) = num_to_roman.get(token_lower.as_str()) {
            return roman.to_string();
        }

        token_lower
    }

    /// Tokenize a string into a set of processed tokens
    fn tokenize(s: &str) -> HashSet<String> {
        let stopwords: HashSet<&str> = ["and", "the", "of", "in", "is", "a", "an"]
            .iter()
            .cloned()
            .collect();

        let s = s.replace(&['_', '-'][..], " ").to_lowercase();
        let s = Self::re_punct().replace_all(&s, " ");

        let mut tokens = HashSet::new();
        for word in s.split_whitespace() {
            if stopwords.contains(word) {
                continue;
            }
            let processed = Self::preprocess_token(word);
            for sub in processed.split_whitespace() {
                tokens.insert(sub.to_string());
            }
        }

        tokens
    }

    /// Levenshtein edit distance between two strings
    fn edit_distance(a: &str, b: &str) -> usize {
        let a_len = a.len();
        let b_len = b.len();
        let mut prev: Vec<usize> = (0..=b_len).collect();
        let mut curr = vec![0usize; b_len + 1];

        for i in 1..=a_len {
            curr[0] = i;
            for j in 1..=b_len {
                let cost = if a.as_bytes()[i - 1] == b.as_bytes()[j - 1] { 0 } else { 1 };
                curr[j] = (prev[j] + 1)
                    .min(curr[j - 1] + 1)
                    .min(prev[j - 1] + cost);
            }
            std::mem::swap(&mut prev, &mut curr);
        }

        prev[b_len]
    }

    /// Check if two tokens match (exact, substring, or fuzzy)
    fn tokens_match(a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        if a.contains(b) || b.contains(a) {
            // Only allow substring match if the shorter token is >= 3 chars
            let shorter = a.len().min(b.len());
            if shorter >= 3 {
                return true;
            }
        }
        // Levenshtein for tokens >= 4 chars, allow distance <= 2
        if a.len() >= 4 && b.len() >= 4 {
            let max_dist = if a.len().min(b.len()) >= 6 { 2 } else { 1 };
            if Self::edit_distance(a, b) <= max_dist {
                return true;
            }
        }
        false
    }

    /// Calculate similarity between ROM tokens (target) and candidate tokens
    fn weighted_similarity(target_tokens: &HashSet<String>, candidate_tokens: &HashSet<String>) -> f32 {
        if target_tokens.is_empty() || candidate_tokens.is_empty() {
            return 0.0;
        }

        let mut matched_target = HashSet::new();

        for t in target_tokens {
            for c in candidate_tokens {
                if Self::tokens_match(t, c) {
                    matched_target.insert(t.clone());
                    break;
                }
            }
        }

        let target_len = target_tokens.len() as f32;
        let candidate_len = candidate_tokens.len() as f32;

        // Base score: fraction of ROM tokens that matched
        let target_coverage = matched_target.len() as f32 / target_len;

        // Penalty for missing ROM tokens (important — these are what the user expects)
        let missing: Vec<_> = target_tokens.difference(&matched_target).collect();
        let missing_penalty: f32 = missing
            .iter()
            .map(|t| if t.as_str() == "i" { 0.0 } else { 0.25 })
            .sum();

        // Small penalty for extra candidate tokens (less important)
        let extra_candidate = (candidate_len - target_len).max(0.0);
        let extra_penalty = extra_candidate * 0.05;

        (target_coverage - missing_penalty - extra_penalty).max(0.0)
    }

    /// Find the best matching image from the cached list
    fn find_image_from_list(&self, sys_name: &str, rom_without_ext: &str) -> Option<String> {
        let cache_entry = self.cache.get(sys_name)?;

        let normalized_rom = Self::normalize_name(rom_without_ext);
        let rom_lower = normalized_rom.to_lowercase();

        // Fast path: exact match after normalization
        for (name, _) in cache_entry {
            let normalized_candidate = Self::normalize_name(&name.replace(".png", "")).to_lowercase();
            if rom_lower == normalized_candidate {
                return Some(name.clone());
            }
        }

        let target_tokens = Self::tokenize(&normalized_rom);
        let mut best_score = 0.0;
        let mut best_candidates = Vec::new();

        for (name, candidate_tokens) in cache_entry {
            let score = Self::weighted_similarity(&target_tokens, candidate_tokens);

            if score > best_score + 0.001 {
                best_score = score;
                best_candidates = vec![name.clone()];
            } else if (score - best_score).abs() <= 0.001 {
                best_candidates.push(name.clone());
            }
        }

        if best_candidates.is_empty() || best_score < 0.4 {
            return None;
        }

        // Preferred region tie-breaker
        if let Some(ref region) = self.preferred_region {
            for candidate in &best_candidates {
                for cap in Self::re_capture_parens().captures_iter(candidate) {
                    if let Some(matched) = cap.get(1) {
                        if matched.as_str().to_uppercase().contains(region) {
                            return Some(candidate.clone());
                        }
                    }
                }
            }
        }

        // Shortest filename tie-breaker
        best_candidates.into_iter().min_by_key(|s| s.len())
    }

    /// Maximum dimensions for boxart images (fits all SpruceOS devices)
    const MAX_WIDTH: u32 = 640;
    const MAX_HEIGHT: u32 = 480;

    /// Download boxart for a single ROM, converting PNG to resized QOI
    pub async fn download_boxart(
        client: &reqwest::Client,
        sys_name: &str,
        image_name: &str,
        dest_path: &Path,
    ) -> Result<(), String> {
        let ra_name = Self::get_ra_alias(sys_name)
            .ok_or_else(|| format!("No Libretro alias found for system: {}", sys_name))?;

        let boxart_url = format!(
            "http://thumbnails.libretro.com/{}/Named_Boxarts/{}",
            ra_name,
            image_name
        )
        .replace(" ", "%20");

        let fallback_url = format!(
            "https://raw.githubusercontent.com/libretro-thumbnails/{}/master/Named_Boxarts/{}",
            ra_name.replace(" ", "_"),
            image_name
        )
        .replace(" ", "%20");

        // Create parent directory if it doesn't exist (if configured to do so)
        if crate::config::SCRAPER_CONFIG.create_missing_dirs {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
            }
        }

        // Try primary URL first, then fallback
        let png_bytes = match Self::download_bytes(client, &boxart_url).await {
            Ok(bytes) => bytes,
            Err(_) => Self::download_bytes(client, &fallback_url).await
                .map_err(|_| "Failed to download from both primary and fallback URLs".to_string())?,
        };

        // Decode PNG, resize, and encode as QOI — all in memory
        let qoi_bytes = tokio::task::spawn_blocking(move || {
            Self::png_to_qoi(&png_bytes)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Image conversion failed: {}", e))?;

        fs::write(dest_path, &qoi_bytes).await.map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Download raw bytes from a URL
    async fn download_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
        let response = client.get(url).send().await.map_err(|e| e.to_string())?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        Ok(bytes.to_vec())
    }

    /// Decode PNG bytes, resize to fit within max dimensions, encode as QOI
    fn png_to_qoi(png_bytes: &[u8]) -> Result<Vec<u8>, String> {
        let img = image::load_from_memory(png_bytes)
            .map_err(|e| e.to_string())?;

        // Only shrink, never enlarge (matches on-device behavior)
        let (w, h) = (img.width(), img.height());
        let img = if w > Self::MAX_WIDTH || h > Self::MAX_HEIGHT {
            img.resize(Self::MAX_WIDTH, Self::MAX_HEIGHT, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };

        // Convert to RGBA (matches on-device QOI pixel format)
        let img = image::DynamicImage::ImageRgba8(img.to_rgba8());

        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, ImageFormat::Qoi)
            .map_err(|e| e.to_string())?;

        Ok(buf.into_inner())
    }

    /// Scrape boxart for only the selected subfolders
    pub async fn scrape_selected_folders(
        &mut self,
        roms_path: &Path,
        folders: &[String],
        progress_tx: mpsc::UnboundedSender<ScrapeProgress>,
    ) -> Result<ScrapeStats, String> {
        let mut stats = ScrapeStats::default();
        let mut tasks = Vec::new();

        for folder in folders {
            let sys_path = roms_path.join(folder);
            if !sys_path.is_dir() {
                continue;
            }
            if Self::get_ra_alias(folder).is_none() {
                continue;
            }
            tasks.extend(self.scan_system_roms(&sys_path, folder).await?);
        }

        stats.total = tasks.len();
        let _ = progress_tx.send(ScrapeProgress::Started { total: stats.total });

        let client = Arc::new(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| e.to_string())?
        );
        let semaphore = Arc::new(tokio::sync::Semaphore::new(
            crate::config::SCRAPER_CONFIG.max_concurrent_downloads
        ));
        let mut handles = Vec::new();

        for (idx, (sys_name, rom_name, dest_path)) in tasks.into_iter().enumerate() {
            let permit = semaphore.clone().acquire_owned().await.map_err(|e| e.to_string())?;
            let image_name = self.find_image_name(&sys_name, &rom_name);
            let progress_tx = progress_tx.clone();
            let client = client.clone();
            let dest_path = dest_path.clone();
            let sys_name = sys_name.clone();

            let handle = tokio::spawn(async move {
                let result = if let Some(img_name) = image_name {
                    BoxArtScraper::download_boxart(&client, &sys_name, &img_name, &dest_path).await
                } else {
                    Err("No matching image found".to_string())
                };

                let status = match &result {
                    Ok(_) => {
                        crate::debug::log(&format!("Boxart scraper: Downloaded {}", rom_name));
                        "Success".to_string()
                    }
                    Err(e) => {
                        crate::debug::log(&format!("Boxart scraper: Failed {} - {}", rom_name, e));
                        format!("Failed: {}", e)
                    }
                };

                let _ = progress_tx.send(ScrapeProgress::Progress {
                    current: idx + 1,
                    rom_name: rom_name.clone(),
                    status,
                });

                drop(permit);
                result
            });

            handles.push(handle);
        }

        for handle in handles {
            match handle.await {
                Ok(Ok(_)) => stats.succeeded += 1,
                Ok(Err(_)) => stats.failed += 1,
                Err(_) => stats.failed += 1,
            }
        }

        let _ = progress_tx.send(ScrapeProgress::Completed(stats.clone()));
        Ok(stats)
    }

    /// Scan a system directory (and subdirectories) for ROMs that need boxart
    async fn scan_system_roms(
        &self,
        sys_path: &Path,
        sys_name: &str,
    ) -> Result<Vec<(String, String, PathBuf)>, String> {
        let mut tasks = Vec::new();

        // Extensions to skip (non-ROM files that may appear in ROM folders)
        let skip_extensions: &[&str] = &[".txt", ".xml", ".json", ".cfg", ".log", ".png", ".jpg", ".bmp", ".qoi", ".db", ".ini"];

        let mut dirs_to_scan = vec![sys_path.to_path_buf()];

        while let Some(dir) = dirs_to_scan.pop() {
            let mut entries = fs::read_dir(&dir)
                .await
                .map_err(|e| format!("Failed to read directory: {}", e))?;

            while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
                let path = entry.path();

                if path.is_dir() {
                    // Skip boxart directories
                    let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    let is_boxart_dir = crate::config::SYSTEM_MAPPINGS
                        .iter()
                        .any(|m| dir_name == m.boxart_subfolder);

                    if !is_boxart_dir {
                        dirs_to_scan.push(path);
                    }
                    continue;
                }

                if !path.is_file() {
                    continue;
                }

                let file_name = path.file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| "Invalid filename".to_string())?;

                // Skip hidden files and known non-ROM extensions
                if file_name.starts_with('.') {
                    continue;
                }
                let lower = file_name.to_lowercase();
                if skip_extensions.iter().any(|ext| lower.ends_with(ext)) {
                    continue;
                }

                // Get ROM name - with or without extension based on config
                let rom_name = if crate::config::BOXART_CONFIG.include_extension_in_name {
                    // Use full filename including extension
                    path.file_name()
                        .and_then(|s| s.to_str())
                        .ok_or_else(|| "Invalid ROM filename".to_string())?
                        .to_string()
                } else {
                    // Use filename without extension (default behavior)
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .ok_or_else(|| "Invalid ROM name".to_string())?
                        .to_string()
                };

                // Get boxart subfolder from system mapping
                let mapping = Self::get_system_mapping(sys_name)
                    .ok_or_else(|| format!("No system mapping for: {}", sys_name))?;
                let imgs_dir = sys_path.join(mapping.boxart_subfolder);

                // Format image filename using config pattern
                let image_filename = crate::config::BOXART_CONFIG.image_name_pattern
                    .replace("{game_name}", &rom_name);
                let image_path = imgs_dir.join(&image_filename);

                // Skip if image already exists (if configured to do so)
                if crate::config::SCRAPER_CONFIG.skip_existing && image_path.exists() {
                    crate::debug::log(&format!("Boxart scraper: Skipped {} (already exists)", rom_name));
                    continue;
                }

                tasks.push((sys_name.to_string(), rom_name, image_path));
            }
        }

        Ok(tasks)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        let tokens = BoxArtScraper::tokenize("Super Mario Bros. 3");
        assert!(tokens.contains("super"));
        assert!(tokens.contains("mario"));
        assert!(tokens.contains("bros"));
        assert!(tokens.contains("iii")); // 3 -> iii
    }

    #[test]
    fn test_normalize_name() {
        let result = BoxArtScraper::normalize_name("Game Name (USA) (Rev 1)");
        assert_eq!(result, "Game Name");
        let result = BoxArtScraper::normalize_name("Game Name [!] [b]");
        assert_eq!(result, "Game Name");
    }

    #[test]
    fn test_get_ra_alias() {
        assert_eq!(BoxArtScraper::get_ra_alias("FC"), Some("Nintendo - Nintendo Entertainment System"));
        assert_eq!(BoxArtScraper::get_ra_alias("GBA"), Some("Nintendo - Game Boy Advance"));
        assert_eq!(BoxArtScraper::get_ra_alias("fc"), Some("Nintendo - Nintendo Entertainment System")); // Case insensitive
        assert_eq!(BoxArtScraper::get_ra_alias("UNKNOWN"), None);
    }

    #[test]
    fn test_weighted_similarity() {
        let tokens1: HashSet<String> = ["super", "mario", "bros"].iter().map(|s| s.to_string()).collect();
        let tokens2: HashSet<String> = ["super", "mario", "brothers"].iter().map(|s| s.to_string()).collect();

        let score = BoxArtScraper::weighted_similarity(&tokens1, &tokens2);
        assert!(score > 0.0);
    }
}
