// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::fs;
use reqwest;

use crate::boxart_db;

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
    /// Create a new BoxArtScraper
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            preferred_region: Some("USA".to_string()), // Default to USA region preference
        }
    }

    /// Create with a custom region preference
    pub fn with_region(region: Option<String>) -> Self {
        Self {
            cache: HashMap::new(),
            preferred_region: region,
        }
    }

    /// Set the preferred region for tie-breaking (e.g., "USA", "Europe", "Japan")
    pub fn set_preferred_region(&mut self, region: Option<String>) {
        self.preferred_region = region;
    }

    /// Get the Libretro alias name for a system
    pub fn get_ra_alias(sys_name: &str) -> Option<&'static str> {
        let mapping: HashMap<&str, &str> = [
            ("AMIGA", "Commodore - Amiga"),
            ("ATARI", "Atari - 2600"),
            ("ATARIST", "Atari - ST"),
            ("ARCADE", "MAME"),
            ("CPS1", "MAME"),
            ("CPS2", "MAME"),
            ("CPS3", "MAME"),
            ("ARDUBOY", "Arduboy Inc - Arduboy"),
            ("CHAI", "ChaiLove"),
            ("COLECO", "Coleco - ColecoVision"),
            ("COMMODORE", "Commodore - 64"),
            ("CPC", "Amstrad - CPC"),
            ("DC", "Sega - Dreamcast"),
            ("DOOM", "DOOM"),
            ("DOS", "DOS"),
            ("EIGHTHUNDRED", "Atari - 8-bit"),
            ("FAIRCHILD", "Fairchild - Channel F"),
            ("FBNEO", "FBNeo - Arcade Games"),
            ("FC", "Nintendo - Nintendo Entertainment System"),
            ("FDS", "Nintendo - Family Computer Disk System"),
            ("FIFTYTWOHUNDRED", "Atari - 5200"),
            ("GB", "Nintendo - Game Boy"),
            ("GBA", "Nintendo - Game Boy Advance"),
            ("GBC", "Nintendo - Game Boy Color"),
            ("GG", "Sega - Game Gear"),
            ("GW", "Handheld Electronic Game"),
            ("INTELLIVISION", "Mattel - Intellivision"),
            ("LYNX", "Atari - Lynx"),
            ("MD", "Sega - Mega Drive - Genesis"),
            ("MS", "Sega - Master System - Mark III"),
            ("MSU1", "Nintendo - Super Nintendo Entertainment System"),
            ("MSUMD", "Sega - Mega Drive - Genesis"),
            ("MSX", "Microsoft - MSX"),
            ("N64", "Nintendo - Nintendo 64"),
            ("NDS", "Nintendo - Nintendo DS"),
            ("NEOCD", "SNK - Neo Geo CD"),
            ("NEOGEO", "SNK - Neo Geo"),
            ("NGP", "SNK - Neo Geo Pocket"),
            ("NGPC", "SNK - Neo Geo Pocket Color"),
            ("ODYSSEY", "Magnavox - Odyssey2"),
            ("PCE", "NEC - PC Engine - TurboGrafx 16"),
            ("PCECD", "NEC - PC Engine CD - TurboGrafx-CD"),
            ("POKE", "Nintendo - Pokemon Mini"),
            ("PS", "Sony - PlayStation"),
            ("PSP", "Sony - PlayStation Portable"),
            ("QUAKE", "Quake"),
            ("SATELLAVIEW", "Nintendo - Satellaview"),
            ("SATURN", "Sega - Saturn"),
            ("SCUMMVM", "ScummVM"),
            ("SEGACD", "Sega - Mega-CD - Sega CD"),
            ("SEGASGONE", "Sega - SG-1000"),
            ("SEVENTYEIGHTHUNDRED", "Atari - 7800"),
            ("SFC", "Nintendo - Super Nintendo Entertainment System"),
            ("SGB", "Nintendo - Game Boy"),
            ("SGFX", "NEC - PC Engine SuperGrafx"),
            ("SUFAMI", "Nintendo - Sufami Turbo"),
            ("SUPERVISION", "Watara - Supervision"),
            ("THIRTYTWOX", "Sega - 32X"),
            ("TIC", "TIC-80"),
            ("VB", "Nintendo - Virtual Boy"),
            ("VECTREX", "GCE - Vectrex"),
            ("VIC20", "Commodore - VIC-20"),
            ("VIDEOPAC", "Philips - Videopac+"),
            ("WOLF", "Wolfenstein 3D"),
            ("WS", "Bandai - WonderSwan"),
            ("WSC", "Bandai - WonderSwan Color"),
            ("X68000", "Sharp - X68000"),
            ("ZXS", "Sinclair - ZX Spectrum"),
        ]
        .iter()
        .cloned()
        .collect();

        mapping.get(sys_name.to_uppercase().as_str()).copied()
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
                    let stripped = Self::strip_parentheses(&name.replace(".png", ""));
                    (name.clone(), Self::tokenize(&stripped))
                })
                .collect();
            self.cache.insert(sys_name.to_string(), tokenized);
        }

        let rom_without_ext = Path::new(rom_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(rom_name);

        self.find_image_from_list(sys_name, rom_without_ext)
    }

    /// Strip parentheses and normalize spaces/symbols
    fn strip_parentheses(s: &str) -> String {
        let re = regex::Regex::new(r"\(.*?\)").unwrap();
        let without_parens = re.replace_all(s, "");
        let re_space = regex::Regex::new(r"[\s\-_,]+").unwrap();
        re_space.replace_all(&without_parens, " ").trim().to_string()
    }

    /// Preprocess a single token (expand abbreviations, convert numbers to roman)
    fn preprocess_token(token: &str) -> String {
        let token_lower = token.to_lowercase();

        // Abbreviation expansion
        let abbreviations: HashMap<&str, &str> = [
            ("ff", "final fantasy"),
            ("zelda", "legend of zelda"),
            ("mario", "super mario"),
        ]
        .iter()
        .cloned()
        .collect();

        if let Some(expansion) = abbreviations.get(token_lower.as_str()) {
            return expansion.to_string();
        }

        // Number to roman numeral conversion
        let num_to_roman: HashMap<&str, &str> = [
            ("2", "ii"), ("3", "iii"), ("4", "iv"), ("5", "v"),
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

    /// Split long tokens for better matching
    fn split_long_token(token: &str) -> HashSet<String> {
        let mut result = HashSet::new();
        let token_lower = token.to_lowercase();

        result.insert(token_lower.clone());

        if token_lower.len() >= 6 && !token_lower.contains(' ') {
            let mid = token_lower.len() / 2;
            result.insert(token_lower[..mid].to_string());
            result.insert(token_lower[mid..].to_string());
        }

        result
    }

    /// Tokenize a string into a set of processed tokens
    fn tokenize(s: &str) -> HashSet<String> {
        let stopwords: HashSet<&str> = ["and", "the", "of", "in", "is", "a", "an"]
            .iter()
            .cloned()
            .collect();

        let s = s.replace(&['_', '-'][..], " ").to_lowercase();
        let re = regex::Regex::new(r"[^\w\s]+").unwrap();
        let s = re.replace_all(&s, " ");

        let mut tokens = HashSet::new();
        for word in s.split_whitespace() {
            if stopwords.contains(word) {
                continue;
            }
            let processed = Self::preprocess_token(word);
            tokens.extend(Self::split_long_token(&processed));
        }

        tokens
    }

    /// Calculate weighted similarity between two token sets
    fn weighted_similarity(target_tokens: &HashSet<String>, candidate_tokens: &HashSet<String>) -> f32 {
        let mut matched_tokens = HashSet::new();

        for t in target_tokens {
            for c in candidate_tokens {
                if t.contains(c) || c.contains(t) {
                    matched_tokens.insert(t.clone());
                    break;
                }
            }
        }

        let missing_tokens: HashSet<_> = target_tokens.difference(&matched_tokens).collect();
        let penalty: f32 = missing_tokens
            .iter()
            .map(|t| if t.as_str() == "1" || t.as_str() == "i" { 0.0 } else { 0.3 })
            .sum();

        let union_size = target_tokens.len().max(candidate_tokens.len());
        if union_size == 0 {
            return 0.0;
        }

        let score = matched_tokens.len() as f32 / union_size as f32;
        (score - penalty).max(0.0)
    }

    /// Find the best matching image from the cached list
    fn find_image_from_list(&self, sys_name: &str, rom_without_ext: &str) -> Option<String> {
        let cache_entry = self.cache.get(sys_name)?;

        let target_tokens = Self::tokenize(&Self::strip_parentheses(rom_without_ext));
        let mut best_score = 0.0;
        let mut best_candidates = Vec::new();

        for (name, candidate_tokens) in cache_entry {
            let score = Self::weighted_similarity(&target_tokens, candidate_tokens);

            if score > best_score {
                best_score = score;
                best_candidates = vec![name.clone()];
            } else if (score - best_score).abs() < 0.001 {
                best_candidates.push(name.clone());
            }
        }

        if best_candidates.is_empty() || best_score < 0.3 {
            return None;
        }

        // Preferred region tie-breaker
        if let Some(ref region) = self.preferred_region {
            for candidate in &best_candidates {
                let re = regex::Regex::new(r"\(([^)]*?)\)").unwrap();
                for cap in re.captures_iter(candidate) {
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

    /// Download boxart for a single ROM
    pub async fn download_boxart(
        &self,
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

        // Create parent directory if it doesn't exist
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
        }

        // Try primary URL first
        if let Ok(()) = Self::download_file(&boxart_url, dest_path).await {
            return Ok(());
        }

        // Try fallback URL
        if let Ok(()) = Self::download_file(&fallback_url, dest_path).await {
            return Ok(());
        }

        Err(format!("Failed to download from both primary and fallback URLs"))
    }

    /// Download a file from a URL to a destination path
    async fn download_file(url: &str, dest_path: &Path) -> Result<(), String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;

        let response = client.get(url).send().await.map_err(|e| e.to_string())?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        fs::write(dest_path, &bytes).await.map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Scrape boxart for all ROMs in a folder
    pub async fn scrape_roms_folder(
        &mut self,
        roms_path: &Path,
        progress_tx: mpsc::UnboundedSender<ScrapeProgress>,
    ) -> Result<ScrapeStats, String> {
        let mut stats = ScrapeStats::default();
        let mut tasks = Vec::new();

        // Scan for systems (subdirectories)
        let mut entries = fs::read_dir(roms_path)
            .await
            .map_err(|e| format!("Failed to read roms directory: {}", e))?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let sys_name = path.file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| "Invalid system directory name".to_string())?
                .to_string();

            // Check if this system has a Libretro alias
            if Self::get_ra_alias(&sys_name).is_none() {
                continue;
            }

            // Scan for ROM files
            tasks.extend(self.scan_system_roms(&path, &sys_name).await?);
        }

        stats.total = tasks.len();
        let _ = progress_tx.send(ScrapeProgress::Started { total: stats.total });

        // Process tasks with concurrency limit (8 workers)
        let semaphore = Arc::new(tokio::sync::Semaphore::new(8));
        let mut handles = Vec::new();

        for (idx, (sys_name, rom_name, dest_path)) in tasks.into_iter().enumerate() {
            let permit = semaphore.clone().acquire_owned().await.map_err(|e| e.to_string())?;
            let image_name = self.find_image_name(&sys_name, &rom_name);
            let progress_tx = progress_tx.clone();
            let dest_path = dest_path.clone();
            let sys_name = sys_name.clone();

            let handle = tokio::spawn(async move {
                let result = if let Some(img_name) = image_name {
                    let scraper = BoxArtScraper::new();
                    scraper.download_boxart(&sys_name, &img_name, &dest_path).await
                } else {
                    Err("No matching image found".to_string())
                };

                let status = match &result {
                    Ok(_) => "Success".to_string(),
                    Err(e) => format!("Failed: {}", e),
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

        // Wait for all tasks to complete
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

    /// Scan a system directory for ROMs that need boxart
    async fn scan_system_roms(
        &self,
        sys_path: &Path,
        sys_name: &str,
    ) -> Result<Vec<(String, String, PathBuf)>, String> {
        let mut tasks = Vec::new();
        let extensions = Self::get_common_extensions(sys_name);

        let mut entries = fs::read_dir(sys_path)
            .await
            .map_err(|e| format!("Failed to read system directory: {}", e))?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let file_name = path.file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| "Invalid filename".to_string())?;

            // Check if file has a supported extension
            if !extensions.iter().any(|ext| file_name.to_lowercase().ends_with(ext)) {
                continue;
            }

            let rom_name = path.file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| "Invalid ROM name".to_string())?
                .to_string();

            let imgs_dir = sys_path.join("Imgs");
            let image_path = imgs_dir.join(format!("{}.png", rom_name));

            // Skip if image already exists
            if image_path.exists() {
                continue;
            }

            tasks.push((sys_name.to_string(), rom_name, image_path));
        }

        Ok(tasks)
    }

    /// Get common ROM extensions for a system
    fn get_common_extensions(sys_name: &str) -> Vec<&'static str> {
        match sys_name.to_uppercase().as_str() {
            "FC" => vec![".nes", ".fds"],
            "SFC" => vec![".sfc", ".smc"],
            "GB" => vec![".gb"],
            "GBC" => vec![".gbc"],
            "GBA" => vec![".gba"],
            "N64" => vec![".n64", ".z64", ".v64"],
            "NDS" => vec![".nds"],
            "MD" => vec![".md", ".gen", ".bin"],
            "MS" => vec![".sms"],
            "GG" => vec![".gg"],
            "PS" => vec![".cue", ".bin", ".iso"],
            "PSP" => vec![".iso", ".cso"],
            "PCE" => vec![".pce"],
            "PCECD" => vec![".cue", ".ccd"],
            "NGP" | "NGPC" => vec![".ngp", ".ngc"],
            "WS" | "WSC" => vec![".ws", ".wsc"],
            "LYNX" => vec![".lnx"],
            "VB" => vec![".vb"],
            "ATARI" => vec![".a26"],
            "COLECO" => vec![".col"],
            _ => vec![".bin", ".rom"],
        }
    }
}

// We need to add regex dependency
// Since we're avoiding adding new dependencies, let's implement a simple regex alternative
mod regex {
    pub struct Regex {
        pattern: String,
    }

    impl Regex {
        pub fn new(pattern: &str) -> Result<Self, String> {
            Ok(Regex {
                pattern: pattern.to_string(),
            })
        }

        pub fn replace_all<'a>(&self, text: &'a str, replacement: &str) -> std::borrow::Cow<'a, str> {
            // Simple implementation for our specific patterns
            if self.pattern == r"\(.*?\)" {
                // Remove content in parentheses
                let mut result = String::new();
                let mut depth: u32 = 0;
                for c in text.chars() {
                    match c {
                        '(' => depth += 1,
                        ')' => depth = depth.saturating_sub(1),
                        _ => if depth == 0 { result.push(c); }
                    }
                }
                std::borrow::Cow::Owned(result)
            } else if self.pattern == r"[\s\-_,]+" {
                // Replace multiple whitespace/symbols with single space
                let mut result = String::new();
                let mut prev_was_space = false;
                for c in text.chars() {
                    if c.is_whitespace() || c == '-' || c == '_' || c == ',' {
                        if !prev_was_space {
                            result.push_str(replacement);
                            prev_was_space = true;
                        }
                    } else {
                        result.push(c);
                        prev_was_space = false;
                    }
                }
                std::borrow::Cow::Owned(result)
            } else if self.pattern == r"[^\w\s]+" {
                // Remove non-word, non-space characters
                let result: String = text.chars()
                    .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
                    .collect();
                std::borrow::Cow::Owned(result)
            } else {
                std::borrow::Cow::Borrowed(text)
            }
        }

        pub fn captures_iter<'r, 't>(&'r self, text: &'t str) -> CapturesIter<'t> {
            CapturesIter {
                text,
                position: 0,
            }
        }
    }

    pub struct CapturesIter<'t> {
        text: &'t str,
        position: usize,
    }

    impl<'t> Iterator for CapturesIter<'t> {
        type Item = Captures<'t>;

        fn next(&mut self) -> Option<Self::Item> {
            // Simple implementation to find text in parentheses
            let remaining = &self.text[self.position..];
            if let Some(start) = remaining.find('(') {
                if let Some(end) = remaining[start..].find(')') {
                    let content = &remaining[start + 1..start + end];
                    self.position += start + end + 1;
                    return Some(Captures {
                        matched: content,
                    });
                }
            }
            None
        }
    }

    pub struct Captures<'t> {
        matched: &'t str,
    }

    impl<'t> Captures<'t> {
        pub fn get(&self, _index: usize) -> Option<Match<'t>> {
            Some(Match {
                text: self.matched,
            })
        }
    }

    pub struct Match<'t> {
        text: &'t str,
    }

    impl<'t> Match<'t> {
        pub fn as_str(&self) -> &'t str {
            self.text
        }
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
    fn test_strip_parentheses() {
        let result = BoxArtScraper::strip_parentheses("Game Name (USA) (Rev 1)");
        assert_eq!(result, "Game Name");
    }

    #[test]
    fn test_get_ra_alias() {
        assert_eq!(BoxArtScraper::get_ra_alias("FC"), Some("Nintendo - Nintendo Entertainment System"));
        assert_eq!(BoxArtScraper::get_ra_alias("GBA"), Some("Nintendo - Game Boy Advance"));
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
