# Boxart Scraper - Quick Reference

## Quick Start

```rust
use crate::boxart_scraper::BoxArtScraper;

// 1. Create scraper
let mut scraper = BoxArtScraper::new();

// 2. Find image for a ROM
let image = scraper.find_image_name("FC", "Super Mario Bros 3.nes");

// 3. Download single boxart
scraper.download_boxart("FC", "Super Mario Bros. 3 (USA).png", dest_path).await?;

// 4. Batch scrape folder
let (tx, rx) = mpsc::unbounded_channel();
scraper.scrape_roms_folder(roms_path, tx).await?;
```

## Key Functions

### `BoxArtScraper::new()`
Creates a new scraper with USA region preference.

### `BoxArtScraper::with_region(region: Option<String>)`
Creates scraper with custom region (e.g., "Europe", "Japan").

### `find_image_name(&mut self, sys_name: &str, rom_name: &str) -> Option<String>`
Finds best matching boxart filename for a ROM.

**Example:**
```rust
scraper.find_image_name("GBA", "Pokemon Emerald.gba")
// Returns: Some("Pokemon - Emerald Version (USA, Europe).png")
```

### `download_boxart(&self, sys_name: &str, image_name: &str, dest_path: &Path) -> Result<(), String>`
Downloads a specific boxart image.

**Example:**
```rust
let dest = Path::new("Roms/GBA/Imgs/Pokemon Emerald.png");
scraper.download_boxart("GBA", "Pokemon - Emerald Version (USA, Europe).png", dest).await?;
```

### `scrape_roms_folder(&mut self, roms_path: &Path, tx: mpsc::UnboundedSender<ScrapeProgress>) -> Result<ScrapeStats, String>`
Scans and downloads boxart for entire ROM folder.

**Example:**
```rust
let (tx, mut rx) = mpsc::unbounded_channel();

tokio::spawn(async move {
    while let Some(progress) = rx.recv().await {
        println!("{:?}", progress);
    }
});

let stats = scraper.scrape_roms_folder(Path::new("Roms"), tx).await?;
println!("Downloaded: {}/{}", stats.succeeded, stats.total);
```

### `get_ra_alias(sys_name: &str) -> Option<&'static str>`
Static function to get Libretro system name.

**Example:**
```rust
BoxArtScraper::get_ra_alias("FC")
// Returns: Some("Nintendo - Nintendo Entertainment System")

BoxArtScraper::get_ra_alias("GBA")
// Returns: Some("Nintendo - Game Boy Advance")
```

## Common System Names

| Code | System | Libretro Name |
|------|--------|---------------|
| FC | NES | Nintendo - Nintendo Entertainment System |
| SFC | SNES | Nintendo - Super Nintendo Entertainment System |
| GB | Game Boy | Nintendo - Game Boy |
| GBC | Game Boy Color | Nintendo - Game Boy Color |
| GBA | Game Boy Advance | Nintendo - Game Boy Advance |
| N64 | Nintendo 64 | Nintendo - Nintendo 64 |
| NDS | Nintendo DS | Nintendo - Nintendo DS |
| MD | Genesis | Sega - Mega Drive - Genesis |
| MS | Master System | Sega - Master System - Mark III |
| GG | Game Gear | Sega - Game Gear |
| PS | PlayStation | Sony - PlayStation |
| PSP | PlayStation Portable | Sony - PlayStation Portable |

See `BOXART_IMPLEMENTATION.md` for complete list of 60 systems.

## ROM Filename Matching Examples

| ROM Filename | Database Match | Score |
|--------------|----------------|-------|
| `Super Mario Bros 3.nes` | `Super Mario Bros. 3 (USA).png` | 0.95 |
| `zelda2.nes` | `Zelda II - The Adventure of Link (USA).png` | 0.85 |
| `ff6.sfc` | `Final Fantasy VI (USA).png` | 0.90 |
| `Pokemon Emerald (U).gba` | `Pokemon - Emerald Version (USA, Europe).png` | 0.92 |
| `DragonBallZ.gba` | `Dragon Ball Z - The Legacy of Goku (USA, Europe).png` | 0.75 |

## Progress Events

```rust
pub enum ScrapeProgress {
    // Sent when scraping starts
    Started { total: usize },

    // Sent for each ROM processed
    Progress {
        current: usize,
        rom_name: String,
        status: String, // "Success" or "Failed: reason"
    },

    // Sent when scraping completes
    Completed(ScrapeStats),
}

pub struct ScrapeStats {
    pub total: usize,      // Total ROMs found
    pub succeeded: usize,  // Successfully downloaded
    pub failed: usize,     // Failed to download
    pub skipped: usize,    // Already had boxart
}
```

## Error Handling

```rust
match scraper.download_boxart(sys, img, path).await {
    Ok(_) => println!("Success!"),
    Err(e) => eprintln!("Error: {}", e),
}

// Common errors:
// - "No Libretro alias found for system: XXX"
// - "HTTP error: 404 Not Found"
// - "Failed to download from both primary and fallback URLs"
// - IO errors (file write, directory creation)
```

## Performance Tips

1. **Reuse scraper instance** - Caches database for each system
2. **Use batch scraping** - 8 concurrent downloads vs sequential
3. **Set region preference** - Reduces tie-breaking overhead
4. **Skip existing** - Auto-skips ROMs with existing boxart
5. **Network** - Ensure stable connection to Libretro servers

## Integration Pattern

```rust
// App state
struct App {
    scraper: Option<BoxArtScraper>,
    progress: Option<ScrapeProgress>,
}

impl App {
    fn start_scrape(&mut self, path: PathBuf) {
        let mut scraper = BoxArtScraper::new();
        let (tx, rx) = mpsc::unbounded_channel();

        // Background task
        tokio::spawn(async move {
            scraper.scrape_roms_folder(&path, tx).await
        });

        // UI updates
        tokio::spawn(async move {
            while let Some(progress) = rx.recv().await {
                // Update UI state
            }
        });
    }
}
```

## Testing

```rust
#[tokio::test]
async fn test_find_match() {
    let mut scraper = BoxArtScraper::new();
    let result = scraper.find_image_name("FC", "Super Mario Bros 3.nes");
    assert!(result.is_some());
    assert!(result.unwrap().contains("Super Mario Bros"));
}

#[tokio::test]
async fn test_download() {
    let scraper = BoxArtScraper::new();
    let temp = tempfile::NamedTempFile::new().unwrap();
    let result = scraper.download_boxart(
        "FC",
        "Super Mario Bros. 3 (USA).png",
        temp.path()
    ).await;
    assert!(result.is_ok());
}
```

## Common Use Cases

### Use Case 1: Single ROM Lookup
```rust
let mut scraper = BoxArtScraper::new();
if let Some(img) = scraper.find_image_name("GBA", "pokemon_emerald.gba") {
    println!("Found: {}", img);
}
```

### Use Case 2: Download Specific Boxart
```rust
let scraper = BoxArtScraper::new();
scraper.download_boxart(
    "N64",
    "Super Mario 64 (USA).png",
    Path::new("output/mario64.png")
).await?;
```

### Use Case 3: Batch Scrape with Progress Bar
```rust
let mut scraper = BoxArtScraper::new();
let (tx, mut rx) = mpsc::unbounded_channel();

// Progress handler
tokio::spawn(async move {
    while let Some(progress) = rx.recv().await {
        match progress {
            ScrapeProgress::Progress { current, rom_name, .. } => {
                println!("[{}/...] Processing: {}", current, rom_name);
            }
            ScrapeProgress::Completed(stats) => {
                println!("Done! {}/{} succeeded", stats.succeeded, stats.total);
            }
            _ => {}
        }
    }
});

// Start scraping
let stats = scraper.scrape_roms_folder(Path::new("Roms"), tx).await?;
```

### Use Case 4: Custom Region Preference
```rust
// Prefer European versions
let mut scraper = BoxArtScraper::with_region(Some("Europe".to_string()));

let img = scraper.find_image_name("MD", "sonic.md");
// Will prefer "Sonic the Hedgehog (Europe).png" over USA version
```

### Use Case 5: System Name Lookup
```rust
// Check if system is supported
if let Some(libretro_name) = BoxArtScraper::get_ra_alias("POKE") {
    println!("System: {}", libretro_name);
    // Output: "Nintendo - Pokemon Mini"
}
```

## Troubleshooting

### No matches found
- Check ROM filename format
- Verify system name is correct (FC, GBA, not NES, GameBoy)
- Check if database exists for system
- Try more generic filename (remove region codes)

### Download fails
- Check network connectivity
- Verify Libretro servers are accessible
- Try fallback GitHub URL manually
- Check destination path permissions

### Slow performance
- Network speed is bottleneck
- Consider reducing concurrent workers
- Check if downloads are being retried

### Memory usage
- Each system database cached in memory
- Clear cache by dropping scraper and creating new one
- Typical usage: 50-100MB for all systems

## URLs

Primary server:
```
http://thumbnails.libretro.com/{system}/Named_Boxarts/{image}
```

Fallback server:
```
https://raw.githubusercontent.com/libretro-thumbnails/{system}/master/Named_Boxarts/{image}
```

## File Structure

Expected:
```
Roms/
├── FC/
│   ├── Game1.nes
│   └── Imgs/
│       └── Game1.png
```

Created automatically:
- `Imgs/` directory in each system folder
- `.png` files matching ROM names

---

**See Also:**
- `BOXART_SCRAPER_USAGE.md` - Full usage guide
- `BOXART_IMPLEMENTATION.md` - Implementation details
- `src/boxart_scraper.rs` - Source code
- `src/boxart_db.rs` - Database loader
