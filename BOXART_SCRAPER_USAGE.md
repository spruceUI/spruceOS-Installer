# Boxart Scraper Usage Guide

The boxart scraper module provides functionality to automatically download boxart for ROM files from the Libretro thumbnails repository.

## Features

- **Fuzzy matching**: Intelligently matches ROM filenames to boxart images using token-based similarity scoring
- **Abbreviation expansion**: Automatically expands common abbreviations (e.g., "ff" → "final fantasy")
- **Roman numeral conversion**: Converts numbers to roman numerals for better matching (e.g., "2" → "ii")
- **Stopword filtering**: Removes common words like "and", "the", "of" for cleaner matching
- **Region preference**: Supports preferred region for tie-breaking when multiple matches exist
- **Concurrent downloads**: Uses 8 parallel workers for fast batch downloads
- **Dual source**: Downloads from thumbnails.libretro.com with GitHub fallback
- **Progress tracking**: Provides real-time progress updates via channels

## Basic Usage

### Initialize the Scraper

```rust
use crate::boxart_scraper::BoxArtScraper;

// Create a new scraper instance
let mut scraper = BoxArtScraper::new();

// Or create with custom region preference
let mut scraper = BoxArtScraper::with_region(Some("Europe".to_string()));
```

### Find Image Name for a ROM

```rust
// Find matching image for a ROM file
let rom_name = "Super Mario Bros 3.nes";
let sys_name = "FC"; // NES system

if let Some(image_name) = scraper.find_image_name(sys_name, rom_name) {
    println!("Found match: {}", image_name);
    // Output: "Super Mario Bros. 3 (USA).png"
}
```

### Download Single Boxart

```rust
use std::path::Path;

let sys_name = "FC";
let image_name = "Super Mario Bros. 3 (USA).png";
let dest_path = Path::new("/path/to/Roms/FC/Imgs/Super Mario Bros 3.png");

match scraper.download_boxart(sys_name, image_name, dest_path).await {
    Ok(_) => println!("Downloaded successfully!"),
    Err(e) => eprintln!("Download failed: {}", e),
}
```

### Batch Scrape Entire ROM Folder

```rust
use tokio::sync::mpsc;
use std::path::Path;
use crate::boxart_scraper::{BoxArtScraper, ScrapeProgress};

#[tokio::main]
async fn main() {
    let mut scraper = BoxArtScraper::new();
    let roms_path = Path::new("/path/to/Roms");

    // Create progress channel
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Spawn task to handle progress updates
    tokio::spawn(async move {
        while let Some(progress) = rx.recv().await {
            match progress {
                ScrapeProgress::Started { total } => {
                    println!("Starting scrape of {} ROMs", total);
                }
                ScrapeProgress::Progress { current, rom_name, status } => {
                    println!("[{}/...] {}: {}", current, rom_name, status);
                }
                ScrapeProgress::Completed(stats) => {
                    println!("\nScrape completed!");
                    println!("Total: {}", stats.total);
                    println!("Succeeded: {}", stats.succeeded);
                    println!("Failed: {}", stats.failed);
                    println!("Skipped: {}", stats.skipped);
                }
            }
        }
    });

    // Start scraping
    match scraper.scrape_roms_folder(roms_path, tx).await {
        Ok(stats) => println!("Scraping finished: {:?}", stats),
        Err(e) => eprintln!("Scraping error: {}", e),
    }
}
```

## Supported Systems

The scraper supports the following systems (mapped to Libretro names):

- **Nintendo**: FC (NES), SFC (SNES), GB, GBC, GBA, N64, NDS, VB, FDS
- **Sega**: MD (Genesis), MS (Master System), GG (Game Gear), SEGACD, DC (Dreamcast), SATURN, THIRTYTWOX (32X)
- **Sony**: PS (PlayStation), PSP
- **NEC**: PCE (TurboGrafx-16), PCECD, SGFX (SuperGrafx)
- **SNK**: NEOGEO, NEOCD, NGP, NGPC
- **Atari**: ATARI (2600), FIFTYTWOHUNDRED (5200), SEVENTYEIGHTHUNDRED (7800), LYNX, EIGHTHUNDRED (8-bit)
- **Others**: ARCADE, DOOM, QUAKE, SCUMMVM, and many more

See `boxart_db::get_supported_systems()` for the complete list.

## ROM File Structure

The scraper expects ROMs to be organized in this structure:

```
Roms/
├── FC/
│   ├── Game1.nes
│   ├── Game2.nes
│   └── Imgs/          # Boxart will be saved here
│       ├── Game1.png
│       └── Game2.png
├── GBA/
│   ├── Game1.gba
│   └── Imgs/
│       └── Game1.png
└── ...
```

The scraper will:
1. Scan each system subdirectory
2. Find ROMs with supported extensions
3. Skip ROMs that already have boxart in the `Imgs/` folder
4. Create `Imgs/` directory if it doesn't exist
5. Download matching boxart as PNG files

## Fuzzy Matching Algorithm

The matching algorithm uses several techniques:

1. **Tokenization**: Splits filenames into words, removes punctuation
2. **Preprocessing**: Expands abbreviations, converts numbers to roman numerals
3. **Stopword removal**: Filters out common words
4. **Long token splitting**: Splits concatenated words (e.g., "dragonball" → "dragon", "ball")
5. **Substring matching**: Allows partial matches between tokens
6. **Weighted scoring**: Calculates similarity with penalties for missing tokens
7. **Region preference**: Breaks ties using preferred region setting
8. **Length tiebreaker**: Chooses shortest filename when scores are equal

### Example Matching

```
ROM: "Super Mario Bros 3 (USA).nes"
  → Tokens: {super, mario, bros, iii}
  → Strips parentheses: "Super Mario Bros 3"

Database: "Super Mario Bros. 3 (USA).png"
  → Tokens: {super, mario, bros, iii}
  → Match score: 0.95 (high similarity)
  → Region match: USA ✓

Result: MATCHED
```

## API Reference

### BoxArtScraper

```rust
pub struct BoxArtScraper {
    cache: HashMap<String, Vec<(String, HashSet<String>)>>,
    preferred_region: Option<String>,
}

impl BoxArtScraper {
    pub fn new() -> Self;
    pub fn with_region(region: Option<String>) -> Self;
    pub fn set_preferred_region(&mut self, region: Option<String>);
    pub fn get_ra_alias(sys_name: &str) -> Option<&'static str>;
    pub fn find_image_name(&mut self, sys_name: &str, rom_name: &str) -> Option<String>;
    pub async fn download_boxart(&self, sys_name: &str, image_name: &str, dest_path: &Path) -> Result<(), String>;
    pub async fn scrape_roms_folder(&mut self, roms_path: &Path, progress_tx: mpsc::UnboundedSender<ScrapeProgress>) -> Result<ScrapeStats, String>;
}
```

### ScrapeStats

```rust
pub struct ScrapeStats {
    pub total: usize,       // Total ROMs to scrape
    pub succeeded: usize,   // Successfully downloaded
    pub failed: usize,      // Failed downloads
    pub skipped: usize,     // Already had boxart
}
```

### ScrapeProgress

```rust
pub enum ScrapeProgress {
    Started { total: usize },
    Progress { current: usize, rom_name: String, status: String },
    Completed(ScrapeStats),
}
```

## Performance

- **Concurrent downloads**: 8 parallel workers
- **Caching**: Image lists are cached per system to avoid re-parsing
- **Timeout**: 30 second timeout per download
- **Embedded database**: All boxart databases are compiled into the binary

## Error Handling

The scraper handles various error conditions:

- **Network errors**: Automatically tries fallback GitHub URL
- **Missing database**: Returns `None` if system database doesn't exist
- **Invalid filenames**: Gracefully handles non-UTF8 filenames
- **File I/O errors**: Reports errors via Result type
- **HTTP errors**: Captures and reports HTTP status codes

## Tips

1. **Network connectivity**: Ensure internet access to thumbnails.libretro.com or github.com
2. **Region preference**: Set preferred region to get your preferred versions (USA/Europe/Japan)
3. **Progress tracking**: Use the progress channel to show UI updates
4. **Partial scraping**: The scraper skips existing images, so it's safe to run multiple times
5. **System names**: System directory names must match the expected format (FC, GBA, etc.)

## Integration Example

```rust
// In your app state
pub struct AppState {
    scraper: Option<BoxArtScraper>,
    scrape_progress: Option<ScrapeProgress>,
}

// Start scraping
pub async fn start_boxart_scrape(&mut self, roms_path: PathBuf) {
    let mut scraper = BoxArtScraper::new();
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Update UI from progress channel
    let progress_handle = tokio::spawn(async move {
        while let Some(progress) = rx.recv().await {
            // Update your UI state here
            // e.g., self.scrape_progress = Some(progress);
        }
    });

    // Run scraper
    let result = scraper.scrape_roms_folder(&roms_path, tx).await;

    // Wait for progress handler
    let _ = progress_handle.await;

    // Handle result
    match result {
        Ok(stats) => println!("Downloaded {} boxarts", stats.succeeded),
        Err(e) => eprintln!("Scrape error: {}", e),
    }
}
```

## License

Copyright (C) 2026 SpruceOS Team
Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)
