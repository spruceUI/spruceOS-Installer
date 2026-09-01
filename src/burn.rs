// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

use std::path::Path;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use flate2::read::GzDecoder;

const CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4MB chunks

/// Chunk size used when pumping bytes out of an archive entry
const ARCHIVE_CHUNK_SIZE: usize = 1024 * 1024; // 1MB

#[derive(Debug, Clone)]
pub enum BurnProgress {
    Started { total_bytes: u64 },
    Writing { written: u64, total: u64 },
    Completed,
    #[allow(dead_code)]
    Cancelled,
    Error(String),
}

/// How a downloaded raw image is compressed.
///
/// BaseOS publishes `.img.zip`; TwigUI publishes `.img.gz`; dArkMoss publishes
/// `.img.7z`, usually split across volumes. All forms are read back as a plain
/// stream of raw image bytes.
///
/// NOTE: `Read` is written fully qualified throughout this section on purpose.
/// Several functions below have their own `use std::io::Read;`, one of which is
/// cfg-gated and guards the statement that follows it — importing `Read` at
/// module scope here would make those look redundant and invite a cleanup that
/// silently breaks the Windows build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageCompression {
    None,
    Gzip,
    Zip,
    SevenZip,
}

fn detect_compression(image_path: &Path) -> ImageCompression {
    let name = image_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    // A multi-volume set is addressed by its first part, so look through any
    // trailing .NNN to the extension that actually describes the format.
    let format_name = crate::github::split_volume_suffix(&name)
        .map_or(name.as_str(), |(base, _)| base);

    if format_name.ends_with(".7z") {
        ImageCompression::SevenZip
    } else if format_name.ends_with(".zip") {
        ImageCompression::Zip
    } else if format_name.ends_with(".gz") {
        ImageCompression::Gzip
    } else {
        ImageCompression::None
    }
}

/// Presents a set of `.7z.001`, `.002`, ... volumes as one continuous stream.
///
/// 7-Zip splits an archive byte-for-byte across volumes with no per-volume
/// header, so concatenating the parts yields a valid archive -- verified by
/// `cat`ing a split set back together and extracting it. This does that
/// concatenation lazily, which is the difference between reading the set in
/// place and writing an 8 GB temp file to read it from.
///
/// A single-file archive is just the one-volume case.
struct MultiVolumeReader {
    files: Vec<std::fs::File>,
    sizes: Vec<u64>,
    total: u64,
    pos: u64,
}

impl MultiVolumeReader {
    fn open(paths: &[std::path::PathBuf]) -> Result<Self, String> {
        let mut files = Vec::with_capacity(paths.len());
        let mut sizes = Vec::with_capacity(paths.len());
        let mut total = 0u64;

        for path in paths {
            let file = std::fs::File::open(path)
                .map_err(|e| format!("Failed to open {:?}: {}", path, e))?;
            let size = file
                .metadata()
                .map_err(|e| format!("Failed to size {:?}: {}", path, e))?
                .len();
            // An empty volume would make the seek arithmetic below spin.
            if size == 0 {
                return Err(format!("Archive volume is empty: {:?}", path));
            }
            total += size;
            files.push(file);
            sizes.push(size);
        }

        if files.is_empty() {
            return Err("No archive volumes to read".to_string());
        }

        Ok(Self { files, sizes, total, pos: 0 })
    }

    fn total(&self) -> u64 {
        self.total
    }
}

impl std::io::Read for MultiVolumeReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        use std::io::{Read as _, Seek as _, SeekFrom};

        if out.is_empty() || self.pos >= self.total {
            return Ok(0);
        }

        // Find the volume holding the current position.
        let mut vol_start = 0u64;
        let mut idx = 0usize;
        while idx < self.sizes.len() && self.pos >= vol_start + self.sizes[idx] {
            vol_start += self.sizes[idx];
            idx += 1;
        }
        if idx >= self.files.len() {
            return Ok(0);
        }

        // Never read past the end of a volume in one go; the next call picks up
        // at the start of the next one.
        let offset_in_vol = self.pos - vol_start;
        let remaining_in_vol = self.sizes[idx] - offset_in_vol;
        let want = std::cmp::min(out.len() as u64, remaining_in_vol) as usize;

        let file = &mut self.files[idx];
        file.seek(SeekFrom::Start(offset_in_vol))?;
        let read = file.read(&mut out[..want])?;
        self.pos += read as u64;
        Ok(read)
    }
}

impl std::io::Seek for MultiVolumeReader {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        use std::io::SeekFrom;

        let target = match pos {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::End(n) => self.total as i64 + n,
            SeekFrom::Current(n) => self.pos as i64 + n,
        };

        if target < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek to a negative position",
            ));
        }

        // Seeking past the end is legal and reads there return 0, matching File.
        self.pos = target as u64;
        Ok(self.pos)
    }
}

/// Every volume backing an image path, in order.
///
/// A path that is not a `.001` yields just itself. Enumeration stops at the
/// first missing part; since a 7z keeps its header in the LAST volume, a set
/// truncated that way fails header parsing rather than silently decompressing
/// a partial image.
fn volume_paths(image_path: &Path) -> Vec<std::path::PathBuf> {
    let name = image_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();

    let Some((base, number)) = crate::github::split_volume_suffix(name) else {
        return vec![image_path.to_path_buf()];
    };
    if number != 1 {
        return vec![image_path.to_path_buf()];
    }

    let dir = image_path.parent().unwrap_or_else(|| Path::new("."));
    let mut paths = Vec::new();
    for n in 1..=999u32 {
        let candidate = dir.join(format!("{}.{:03}", base, n));
        if !candidate.exists() {
            break;
        }
        paths.push(candidate);
    }

    if paths.is_empty() {
        vec![image_path.to_path_buf()]
    } else {
        paths
    }
}

/// Name and uncompressed size of the image inside a 7z archive.
///
/// The largest entry with data -- the raw `.img` in every archive we ship, and
/// robust against a stray readme sitting alongside it. The size comes straight
/// from the archive header, so unlike the `.gz` path this needs no pre-scan.
fn seven_zip_image_entry(volume_paths: &[std::path::PathBuf]) -> Result<(String, u64), String> {
    let mut reader = MultiVolumeReader::open(volume_paths)?;
    let len = reader.total();
    let password = sevenz_rust::Password::empty();

    let archive = sevenz_rust::Archive::read(&mut reader, len, password.as_slice())
        .map_err(|e| format!("Failed to read 7z archive header: {}", e))?;

    let entry = archive
        .files
        .iter()
        .filter(|f| f.has_stream && !f.is_directory && f.size > 0)
        .max_by_key(|f| f.size)
        .ok_or_else(|| "7z archive contains no files".to_string())?;

    Ok((entry.name.clone(), entry.size))
}

/// A `Read` fed by a worker thread over a bounded channel.
///
/// Both archive readers below need this shape: the underlying library hands
/// out entry data borrowed from the archive, or through a callback, and
/// neither can escape into the `Box<dyn Read>` the burn paths expect. A worker
/// thread owns the archive instead and pushes chunks across; the channel bound
/// caps how much decompressed data buffers ahead of the writer.
struct ChannelReader {
    rx: std::sync::mpsc::Receiver<Result<Vec<u8>, String>>,
    current: Vec<u8>,
    pos: usize,
    finished: bool,
}

impl ChannelReader {
    fn new(rx: std::sync::mpsc::Receiver<Result<Vec<u8>, String>>) -> Self {
        Self { rx, current: Vec::new(), pos: 0, finished: false }
    }
}

impl std::io::Read for ChannelReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.pos >= self.current.len() {
            if self.finished {
                return Ok(0);
            }
            match self.rx.recv() {
                Ok(Ok(chunk)) => {
                    self.current = chunk;
                    self.pos = 0;
                }
                Ok(Err(e)) => {
                    self.finished = true;
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e));
                }
                // Sender dropped without an error: the entry was fully read
                Err(_) => {
                    self.finished = true;
                    return Ok(0);
                }
            }
        }

        let n = std::cmp::min(self.current.len() - self.pos, out.len());
        out[..n].copy_from_slice(&self.current[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Streams the first entry of a zip archive as a plain `Read`.
fn spawn_zip_reader(image_path: &Path) -> Result<ChannelReader, String> {
    // Validate the archive up front so a bad file fails here rather than
    // partway through writing to the device.
    let entry_name = {
        let file = std::fs::File::open(image_path)
            .map_err(|e| format!("Failed to open zip image: {}", e))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| format!("Failed to read zip image: {}", e))?;
        if archive.is_empty() {
            return Err("Zip image contains no files".to_string());
        }
        let entry = archive
            .by_index(0)
            .map_err(|e| format!("Failed to open first zip entry: {}", e))?;
        entry.name().to_string()
    };

    crate::debug::log(&format!("Zip entry: {}", entry_name));

    let path = image_path.to_path_buf();
    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<Vec<u8>, String>>(4);

    std::thread::spawn(move || {
        use std::io::Read;

        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                let _ = tx.send(Err(format!("Failed to open zip image: {}", e)));
                return;
            }
        };
        let mut archive = match zip::ZipArchive::new(file) {
            Ok(a) => a,
            Err(e) => {
                let _ = tx.send(Err(format!("Failed to read zip image: {}", e)));
                return;
            }
        };
        let mut entry = match archive.by_index(0) {
            Ok(e) => e,
            Err(e) => {
                let _ = tx.send(Err(format!("Failed to open first zip entry: {}", e)));
                return;
            }
        };

        let mut buffer = vec![0u8; ARCHIVE_CHUNK_SIZE];
        loop {
            match entry.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    // A send error means the reader was dropped (cancelled burn)
                    if tx.send(Ok(buffer[..n].to_vec())).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("Failed to decompress zip image: {}", e)));
                    return;
                }
            }
        }
    });

    Ok(ChannelReader::new(rx))
}

/// Streams the image entry out of a 7z archive, single- or multi-volume.
///
/// This is what keeps the 7z burn path honest: the bundled 7-Zip binary can
/// only unpack to a file first, which would put an 8 GB scratch requirement on
/// every user. `sevenz-rust` decompresses as it reads, so the image goes
/// straight from the archive to the card the same way `.gz` and `.zip` do.
fn spawn_7z_reader(volumes: Vec<std::path::PathBuf>) -> Result<ChannelReader, String> {
    // Read the header first, so a truncated or unreadable set fails before the
    // device is touched.
    let (entry_name, entry_size) = seven_zip_image_entry(&volumes)?;
    crate::debug::log(&format!("7z entry: {} ({} bytes)", entry_name, entry_size));

    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<Vec<u8>, String>>(4);

    std::thread::spawn(move || {
        use std::io::Read;

        let reader = match MultiVolumeReader::open(&volumes) {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(Err(e));
                return;
            }
        };
        let len = reader.total();

        let mut archive = match sevenz_rust::SevenZReader::new(
            reader,
            len,
            sevenz_rust::Password::empty(),
        ) {
            Ok(a) => a,
            Err(e) => {
                let _ = tx.send(Err(format!("Failed to read 7z image: {}", e)));
                return;
            }
        };

        // `for_each_entries` walks every entry; returning false stops the walk.
        let result = archive.for_each_entries(|entry, entry_reader| {
            if entry.name != entry_name {
                // Entries in a solid block are slices of one decompressed
                // stream. Skipping one without reading it leaves the stream
                // short, and the NEXT entry then decodes from the wrong offset
                // -- silently, as plausible-looking garbage. Drain it instead.
                std::io::copy(entry_reader, &mut std::io::sink())?;
                return Ok(true);
            }

            let mut buffer = vec![0u8; ARCHIVE_CHUNK_SIZE];
            loop {
                let n = entry_reader.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                // A send error means the reader was dropped (cancelled burn)
                if tx.send(Ok(buffer[..n].to_vec())).is_err() {
                    return Ok(false);
                }
            }
            Ok(false)
        });

        if let Err(e) = result {
            let _ = tx.send(Err(format!("Failed to decompress 7z image: {}", e)));
        }
    });

    Ok(ChannelReader::new(rx))
}

/// Opens an image as a stream of raw bytes, decompressing `.gz`, `.zip` and
/// `.7z` on the fly so the burn paths always see a plain image.
fn open_image_reader(image_path: &Path) -> Result<Box<dyn std::io::Read>, String> {
    match detect_compression(image_path) {
        ImageCompression::Zip => {
            crate::debug::log("Detected .zip image, decompressing on-the-fly during burn");
            Ok(Box::new(spawn_zip_reader(image_path)?))
        }
        ImageCompression::SevenZip => {
            let volumes = volume_paths(image_path);
            crate::debug::log(&format!(
                "Detected .7z image ({} volume(s)), decompressing on-the-fly during burn",
                volumes.len()
            ));
            Ok(Box::new(spawn_7z_reader(volumes)?))
        }
        ImageCompression::Gzip => {
            crate::debug::log("Detected .gz image, decompressing on-the-fly during burn");
            let file = std::fs::File::open(image_path)
                .map_err(|e| format!("Failed to open image file: {}", e))?;
            Ok(Box::new(GzDecoder::new(file)))
        }
        ImageCompression::None => {
            let file = std::fs::File::open(image_path)
                .map_err(|e| format!("Failed to open image file: {}", e))?;
            Ok(Box::new(file))
        }
    }
}

/// Burns a raw disk image to a device and verifies the write
pub async fn burn_image(
    image_path: &Path,
    device_path: &str,
    progress_tx: UnboundedSender<BurnProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    crate::debug::log_section("Image Burning");
    crate::debug::log(&format!("Image: {:?}", image_path));
    crate::debug::log(&format!("Device: {}", device_path));

    // Get image size - for compressed files, we need the decompressed size
    let compressed_size = tokio::fs::metadata(image_path)
        .await
        .map_err(|e| format!("Failed to get image size: {}", e))?
        .len();

    let compression = detect_compression(image_path);

    let image_size = if compression == ImageCompression::Zip {
        crate::debug::log(&format!("Compressed size: {} bytes ({:.2} GB)", compressed_size, compressed_size as f64 / 1_073_741_824.0));

        // Zip records the uncompressed size in its headers, so unlike .gz this
        // needs no pre-scan of the whole file.
        let decompressed_size = tokio::task::spawn_blocking({
            let image_path = image_path.to_path_buf();
            move || -> Result<u64, String> {
                let file = std::fs::File::open(&image_path)
                    .map_err(|e| format!("Failed to open image for size check: {}", e))?;
                let mut archive = zip::ZipArchive::new(file)
                    .map_err(|e| format!("Failed to read zip image: {}", e))?;
                if archive.is_empty() {
                    return Err("Zip image contains no files".to_string());
                }
                let entry = archive
                    .by_index(0)
                    .map_err(|e| format!("Failed to open first zip entry: {}", e))?;
                Ok(entry.size())
            }
        }).await
        .map_err(|e| format!("Size lookup task failed: {}", e))??;

        if decompressed_size == 0 {
            return Err("Zip image reports an uncompressed size of zero".to_string());
        }

        crate::debug::log(&format!("Decompressed size: {} bytes ({:.2} GB)", decompressed_size, decompressed_size as f64 / 1_073_741_824.0));
        decompressed_size
    } else if compression == ImageCompression::SevenZip {
        // Sum the volumes rather than stat the first part, so the log reports
        // the real on-disk size of the set.
        let volumes = volume_paths(image_path);
        let archive_size: u64 = volumes
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len()))
            .sum();
        crate::debug::log(&format!(
            "Compressed size: {} bytes ({:.2} GB) across {} volume(s)",
            archive_size,
            archive_size as f64 / 1_073_741_824.0,
            volumes.len()
        ));

        // The 7z header records the uncompressed size, so this needs no
        // pre-scan of the archive -- unlike the .gz path below.
        let decompressed_size = tokio::task::spawn_blocking(move || -> Result<u64, String> {
            let (name, size) = seven_zip_image_entry(&volumes)?;
            crate::debug::log(&format!("7z image entry: {} ({} bytes)", name, size));
            Ok(size)
        }).await
        .map_err(|e| format!("Size lookup task failed: {}", e))??;

        if decompressed_size == 0 {
            return Err("7z image reports an uncompressed size of zero".to_string());
        }

        crate::debug::log(&format!("Decompressed size: {} bytes ({:.2} GB)", decompressed_size, decompressed_size as f64 / 1_073_741_824.0));
        decompressed_size
    } else if compression == ImageCompression::Gzip {
        crate::debug::log(&format!("Compressed size: {} bytes ({:.2} GB)", compressed_size, compressed_size as f64 / 1_073_741_824.0));
        crate::debug::log("Pre-scanning .gz file to determine decompressed size...");

        // Determine decompressed size by reading through the file
        let decompressed_size = tokio::task::spawn_blocking({
            let image_path = image_path.to_path_buf();
            move || -> Result<u64, String> {
                use std::io::Read;
                let file = std::fs::File::open(&image_path)
                    .map_err(|e| format!("Failed to open image for size check: {}", e))?;
                let mut decoder = GzDecoder::new(file);
                let mut total = 0u64;
                let mut buffer = vec![0u8; 1024 * 1024]; // 1MB buffer for faster scanning

                loop {
                    let bytes_read = decoder.read(&mut buffer)
                        .map_err(|e| format!("Failed to read compressed image: {}", e))?;
                    if bytes_read == 0 {
                        break;
                    }
                    total += bytes_read as u64;
                }

                Ok(total)
            }
        }).await
        .map_err(|e| format!("Size scan task failed: {}", e))??;

        crate::debug::log(&format!("Decompressed size: {} bytes ({:.2} GB)", decompressed_size, decompressed_size as f64 / 1_073_741_824.0));
        decompressed_size
    } else {
        crate::debug::log(&format!("Image size: {} bytes ({:.2} GB)", compressed_size, compressed_size as f64 / 1_073_741_824.0));
        compressed_size
    };

    let _ = progress_tx.send(BurnProgress::Started { total_bytes: image_size });

    // Unmount the device first
    unmount_device(device_path).await?;

    // Platform-specific burn implementation
    // Returns the actual number of bytes written (decompressed size)
    #[cfg(target_os = "windows")]
    let result = burn_image_windows(image_path, device_path, image_size, &progress_tx, &cancel_token).await;

    #[cfg(target_os = "linux")]
    let result = burn_image_linux(image_path, device_path, image_size, &progress_tx, &cancel_token).await;

    #[cfg(target_os = "macos")]
    let result = burn_image_macos(image_path, device_path, image_size, &progress_tx, &cancel_token).await;

    match result {
        Ok(actual_bytes_written) => {
            crate::debug::log(&format!("Actual bytes written: {} ({:.2} GB)", actual_bytes_written, actual_bytes_written as f64 / 1_073_741_824.0));

            // No read-back verification. It only ever ran on Linux -- Windows and
            // macOS returned an empty hash and skipped the comparison -- while the
            // image-hash half ran everywhere and was discarded on those platforms.
            // That cost a second full decompression pass, silently, after the write
            // had already finished. Dropped deliberately; see git history if it
            // needs to come back.
            let _ = progress_tx.send(BurnProgress::Completed);
            crate::debug::log("Image burn complete");
            Ok(())
        }
        Err(e) => {
            let _ = progress_tx.send(BurnProgress::Error(e.clone()));
            Err(e)
        }
    }
}

/// Unmount all partitions on the device
async fn unmount_device(device_path: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // On Windows, unmount all volumes on this physical drive
        crate::debug::log("Unmounting volumes on Windows...");
        unmount_device_windows(device_path).await
    }

    #[cfg(target_os = "linux")]
    {
        // On Linux, unmount all partitions (e.g., /dev/sdb1, /dev/sdb2, etc.)
        crate::debug::log("Unmounting partitions on Linux...");
        unmount_device_linux(device_path).await
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, use diskutil
        crate::debug::log("Unmounting disk on macOS...");
        unmount_device_macos(device_path).await
    }
}

// =============================================================================
// Windows Implementation
// =============================================================================

#[cfg(target_os = "windows")]
async fn unmount_device_windows(_device_path: &str) -> Result<(), String> {
    // Volume locking is now handled inside burn_image_windows to keep handles open
    Ok(())
}

#[cfg(target_os = "windows")]
async fn burn_image_windows(
    image_path: &Path,
    device_path: &str,
    image_size: u64,
    progress_tx: &UnboundedSender<BurnProgress>,
    cancel_token: &CancellationToken,
) -> Result<u64, String> {
    // Device path should already be in \\.\PhysicalDriveN format from drives.rs
    crate::debug::log(&format!("Opening physical drive: {}", device_path));

    // Move ALL Windows API operations into spawn_blocking since HANDLE is !Send
    let bytes_written = tokio::task::spawn_blocking({
        let image_path = image_path.to_path_buf();
        let device_path = device_path.to_string();
        let progress_tx = progress_tx.clone();
        let cancel_token = cancel_token.clone();

        move || -> Result<u64, String> {
            use windows::Win32::Foundation::*;
            use windows::Win32::Storage::FileSystem::*;
            use windows::Win32::System::IO::*;
            use windows::Win32::System::Ioctl::*;
            use std::io::Read;

            // Import IOCTL for getting device number
            const IOCTL_STORAGE_GET_DEVICE_NUMBER: u32 = 0x002D1080;

            // CRITICAL: Lock and dismount volumes on the TARGET PHYSICAL DRIVE and KEEP HANDLES OPEN
            // This prevents Windows from auto-mounting partitions during the burn

            // Extract physical drive number from device_path (e.g., "3" from "\\.\PhysicalDrive3")
            let target_drive_number: Option<u32> = device_path
                .strip_prefix("\\\\.\\PhysicalDrive")
                .and_then(|s| s.parse::<u32>().ok());

            crate::debug::log(&format!("Locking volumes on target physical drive: {:?}", target_drive_number));
            let mut volume_handles: Vec<HANDLE> = Vec::new();

            #[repr(C)]
            struct STORAGE_DEVICE_NUMBER {
                device_type: u32,
                device_number: u32,
                partition_number: u32,
            }

            unsafe {
                let drive_bits = GetLogicalDrives();
                for i in 0..26u8 {
                    if (drive_bits >> i) & 1 == 1 {
                        let letter = (b'A' + i) as char;
                        let volume_path: Vec<u16> = format!("\\\\.\\{}:", letter)
                            .encode_utf16()
                            .chain(Some(0))
                            .collect();

                        // Open the volume to check which physical drive it belongs to
                        let check_handle = CreateFileW(
                            windows::core::PCWSTR(volume_path.as_ptr()),
                            GENERIC_READ.0,
                            FILE_SHARE_READ | FILE_SHARE_WRITE,
                            None,
                            OPEN_EXISTING,
                            Default::default(),
                            None,
                        );

                        if let Ok(check_handle) = check_handle {
                            let mut device_number = STORAGE_DEVICE_NUMBER {
                                device_type: 0,
                                device_number: 0,
                                partition_number: 0,
                            };
                            let mut bytes_returned: u32 = 0;

                            let result = DeviceIoControl(
                                check_handle,
                                IOCTL_STORAGE_GET_DEVICE_NUMBER,
                                None,
                                0,
                                Some(&mut device_number as *mut _ as *mut _),
                                std::mem::size_of::<STORAGE_DEVICE_NUMBER>() as u32,
                                Some(&mut bytes_returned),
                                None,
                            );

                            let _ = CloseHandle(check_handle);

                            // Only lock volumes that match our target physical drive
                            if result.is_ok() && Some(device_number.device_number) == target_drive_number {
                                crate::debug::log(&format!("Volume {}: belongs to target physical drive {}", letter, device_number.device_number));

                                // Re-open with write access for locking
                                let vol_handle = CreateFileW(
                                    windows::core::PCWSTR(volume_path.as_ptr()),
                                    FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
                                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                                    None,
                                    OPEN_EXISTING,
                                    Default::default(),
                                    None,
                                );

                                if let Ok(vol_handle) = vol_handle {
                                    let mut bytes_returned: u32 = 0;

                                    // Lock the volume
                                    let _ = DeviceIoControl(
                                        vol_handle,
                                        FSCTL_LOCK_VOLUME,
                                        None,
                                        0,
                                        None,
                                        0,
                                        Some(&mut bytes_returned),
                                        None,
                                    );

                                    // Dismount the volume
                                    let dismount_result = DeviceIoControl(
                                        vol_handle,
                                        FSCTL_DISMOUNT_VOLUME,
                                        None,
                                        0,
                                        None,
                                        0,
                                        Some(&mut bytes_returned),
                                        None,
                                    );

                                    if dismount_result.is_ok() {
                                        crate::debug::log(&format!("Locked/dismounted {}:", letter));
                                    }

                                    // KEEP THE HANDLE OPEN - add to list for cleanup later
                                    volume_handles.push(vol_handle);
                                }
                            }
                        }
                    }
                }
            }

            // Helper function to cleanup volume handles
            let cleanup_volumes = |volume_handles: &Vec<HANDLE>| {
                unsafe {
                    for vol_handle in volume_handles {
                        let mut bytes_returned: u32 = 0;
                        let _ = DeviceIoControl(
                            *vol_handle,
                            FSCTL_UNLOCK_VOLUME,
                            None,
                            0,
                            None,
                            0,
                            Some(&mut bytes_returned),
                            None,
                        );
                        let _ = CloseHandle(*vol_handle);
                    }
                }
            };

            let device_path_wide: Vec<u16> = device_path
                .encode_utf16()
                .chain(Some(0))
                .collect();

            // Open the physical drive for writing with exclusive access
            // FILE_FLAG_WRITE_THROUGH: Ensures data is physically written before returning
            // FILE_SHARE_MODE(0): Exclusive access - no sharing allowed
            // Note: Not using FILE_FLAG_NO_BUFFERING for now to avoid parameter errors
            let handle = unsafe {
                CreateFileW(
                    windows::core::PCWSTR(device_path_wide.as_ptr()),
                    FILE_GENERIC_WRITE.0 | FILE_GENERIC_READ.0,
                    FILE_SHARE_MODE(0), // Exclusive access - no sharing
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_WRITE_THROUGH,
                    None,
                )
            };

            if handle.is_err() {
                let err = windows::core::Error::from_win32();
                cleanup_volumes(&volume_handles);
                return Err(format!("Failed to open device for writing: {:?}", err));
            }

            let handle = handle.unwrap();
            crate::debug::log("Physical drive opened successfully");

            // CRITICAL: Wipe the first 1MB of the disk to clear partition tables
            // This prevents Windows from auto-mounting partitions as we write them,
            // which would cause access denied errors partway through the burn
            const WIPE_SIZE: usize = 1 * 1024 * 1024; // 1 MB
            const SECTOR_SIZE: usize = 512;
            let wipe_size_aligned = ((WIPE_SIZE + SECTOR_SIZE - 1) / SECTOR_SIZE) * SECTOR_SIZE;
            let wipe_buffer = vec![0u8; wipe_size_aligned];

            crate::debug::log(&format!("Wiping first {} bytes to clear partition table...", wipe_size_aligned));

            let mut bytes_written = 0u32;
            unsafe {
                let wipe_result = WriteFile(
                    handle,
                    Some(&wipe_buffer),
                    Some(&mut bytes_written),
                    None,
                );

                if wipe_result.is_err() || bytes_written as usize != wipe_size_aligned {
                    let err = windows::core::Error::from_win32();
                    let _ = CloseHandle(handle);
                    cleanup_volumes(&volume_handles);
                    return Err(format!("Failed to wipe partition table: {:?}", err));
                }
            }

            crate::debug::log(&format!("Partition table wiped ({} bytes), resetting file pointer...", bytes_written));

            // Reset file pointer to the beginning of the disk
            unsafe {
                use windows::Win32::Storage::FileSystem::SetFilePointerEx;
                use windows::Win32::Storage::FileSystem::FILE_BEGIN;

                let seek_result = SetFilePointerEx(
                    handle,
                    0,
                    None,
                    FILE_BEGIN,
                );

                if seek_result.is_err() {
                    let _ = CloseHandle(handle);
                    cleanup_volumes(&volume_handles);
                    return Err("Failed to reset file pointer after wipe".to_string());
                }
            }

            crate::debug::log("File pointer reset, beginning image write...");

            let mut image_reader = open_image_reader(&image_path)
                .map_err(|e| {
                    unsafe { let _ = CloseHandle(handle); }
                    cleanup_volumes(&volume_handles);
                    e
                })?;

            // Windows requires 512-byte sector-aligned writes for physical drives (SECTOR_SIZE already defined above)
            // Allocate buffers: read buffer for decompression, sector buffer for aligned writes
            let mut read_buffer = vec![0u8; CHUNK_SIZE];
            // Sector buffer must hold at least CHUNK_SIZE rounded up to next sector boundary
            let sector_buffer_size = ((CHUNK_SIZE + SECTOR_SIZE - 1) / SECTOR_SIZE) * SECTOR_SIZE + SECTOR_SIZE;
            let mut sector_buffer = vec![0u8; sector_buffer_size];
            let mut sector_buffer_pos = 0usize;
            let mut total_written = 0u64;

            loop {
                if cancel_token.is_cancelled() {
                    crate::debug::log("Burn cancelled by user");
                    unsafe {
                        let _ = CloseHandle(handle);
                    }
                    cleanup_volumes(&volume_handles);
                    return Err("Burn cancelled".to_string());
                }

                // Read from image into read_buffer
                let bytes_read = image_reader.read(&mut read_buffer)
                    .map_err(|e| {
                        unsafe { let _ = CloseHandle(handle); }
                        cleanup_volumes(&volume_handles);
                        format!("Failed to read from image: {}", e)
                    })?;

                if bytes_read == 0 {
                    // EOF - handle any remaining partial sector
                    if sector_buffer_pos > 0 {
                        // Pad to sector boundary with zeros
                        let padded_size = ((sector_buffer_pos + SECTOR_SIZE - 1) / SECTOR_SIZE) * SECTOR_SIZE;
                        sector_buffer[sector_buffer_pos..padded_size].fill(0);

                        crate::debug::log(&format!("Writing final sector: {} bytes (padded from {})", padded_size, sector_buffer_pos));

                        let mut bytes_written = 0u32;
                        unsafe {
                            let write_result = WriteFile(
                                handle,
                                Some(&sector_buffer[..padded_size]),
                                Some(&mut bytes_written),
                                None,
                            );

                            if write_result.is_err() {
                                let err = windows::core::Error::from_win32();
                                let _ = CloseHandle(handle);
                                cleanup_volumes(&volume_handles);
                                return Err(format!("Failed to write final sector at offset {}: {:?}", total_written, err));
                            }
                        }

                        total_written += sector_buffer_pos as u64; // Count only actual data, not padding
                    }
                    break;
                }

                // Append new data to sector buffer
                sector_buffer[sector_buffer_pos..sector_buffer_pos + bytes_read]
                    .copy_from_slice(&read_buffer[..bytes_read]);
                sector_buffer_pos += bytes_read;

                // Write all complete sectors
                let sectors_to_write = (sector_buffer_pos / SECTOR_SIZE) * SECTOR_SIZE;
                if sectors_to_write > 0 {
                    let mut bytes_written = 0u32;
                    unsafe {
                        let write_result = WriteFile(
                            handle,
                            Some(&sector_buffer[..sectors_to_write]),
                            Some(&mut bytes_written),
                            None,
                        );

                        if write_result.is_err() {
                            let err = windows::core::Error::from_win32();
                            let _ = CloseHandle(handle);
                            cleanup_volumes(&volume_handles);
                            return Err(format!("Failed to write to device at offset {}: {:?}", total_written, err));
                        }

                        if bytes_written as usize != sectors_to_write {
                            let _ = CloseHandle(handle);
                            cleanup_volumes(&volume_handles);
                            return Err(format!("Incomplete write at offset {}: wrote {} of {} bytes",
                                total_written, bytes_written, sectors_to_write));
                        }
                    }

                    total_written += bytes_written as u64;
                    let _ = progress_tx.send(BurnProgress::Writing {
                        written: total_written,
                        total: image_size,
                    });

                    // Move remaining partial sector to start of buffer
                    let remaining = sector_buffer_pos - sectors_to_write;
                    if remaining > 0 {
                        sector_buffer.copy_within(sectors_to_write..sector_buffer_pos, 0);
                    }
                    sector_buffer_pos = remaining;
                }
            }

            // Flush buffers
            unsafe {
                let _ = FlushFileBuffers(handle);
            }

            crate::debug::log(&format!("Write complete: {} bytes written", total_written));

            // Close the physical drive handle
            unsafe {
                let _ = CloseHandle(handle);
            }

            // Unlock and close all volume handles
            crate::debug::log("Unlocking and closing volume handles...");
            cleanup_volumes(&volume_handles);
            crate::debug::log("Device closed, volumes unlocked");
            Ok(total_written)
        }
    })
    .await
    .map_err(|e| format!("Write task failed: {}", e))?;

    bytes_written
}

// =============================================================================
// Linux Implementation
// =============================================================================

#[cfg(target_os = "linux")]
async fn unmount_device_linux(device_path: &str) -> Result<(), String> {
    use tokio::process::Command;

    // Find all mounted partitions for this device
    let mount_output = Command::new("mount")
        .output()
        .await
        .map_err(|e| format!("Failed to run mount command: {}", e))?;

    let mount_str = String::from_utf8_lossy(&mount_output.stdout);

    // Find all partitions (e.g., /dev/sdb1, /dev/sdb2)
    for line in mount_str.lines() {
        if line.starts_with(device_path) {
            if let Some(partition) = line.split_whitespace().next() {
                crate::debug::log(&format!("Unmounting partition: {}", partition));
                let result = Command::new("umount")
                    .arg(partition)
                    .output()
                    .await;

                match result {
                    Ok(output) if output.status.success() => {
                        crate::debug::log(&format!("Successfully unmounted {}", partition));
                    }
                    _ => {
                        crate::debug::log(&format!("Warning: Could not unmount {}", partition));
                    }
                }
            }
        }
    }

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn burn_image_linux(
    image_path: &Path,
    device_path: &str,
    image_size: u64,
    progress_tx: &UnboundedSender<BurnProgress>,
    cancel_token: &CancellationToken,
) -> Result<u64, String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::io::{Read, Write};

    crate::debug::log(&format!("Opening device: {}", device_path));

    let bytes_written = tokio::task::spawn_blocking({
        let image_path = image_path.to_path_buf();
        let device_path = device_path.to_string();
        let progress_tx = progress_tx.clone();
        let cancel_token = cancel_token.clone();

        move || -> Result<u64, String> {
            // Open device with O_WRONLY | O_SYNC | O_DIRECT flags
            let mut device = std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_SYNC)
                .open(&device_path)
                .map_err(|e| format!("Failed to open device {}: {}. Are you running with sudo/root?", device_path, e))?;

            let mut image_reader = open_image_reader(&image_path)?;

            let mut buffer = vec![0u8; CHUNK_SIZE];
            let mut total_written = 0u64;

            loop {
                if cancel_token.is_cancelled() {
                    crate::debug::log("Burn cancelled by user");
                    return Err("Burn cancelled".to_string());
                }

                let bytes_read = image_reader.read(&mut buffer)
                    .map_err(|e| format!("Failed to read from image: {}", e))?;

                if bytes_read == 0 {
                    break; // EOF
                }

                device.write_all(&buffer[..bytes_read])
                    .map_err(|e| format!("Failed to write to device at offset {}: {}", total_written, e))?;

                total_written += bytes_read as u64;
                let _ = progress_tx.send(BurnProgress::Writing {
                    written: total_written,
                    total: image_size,
                });
            }

            // Sync to ensure all data is written
            device.sync_all()
                .map_err(|e| format!("Failed to sync device: {}", e))?;

            crate::debug::log(&format!("Write complete: {} bytes written", total_written));
            Ok(total_written)
        }
    })
    .await
    .map_err(|e| format!("Write task failed: {}", e))?;

    bytes_written
}

// =============================================================================
// macOS Implementation
// =============================================================================

#[cfg(target_os = "macos")]
async fn unmount_device_macos(device_path: &str) -> Result<(), String> {
    use tokio::process::Command;
    use tokio::time::{timeout, Duration};

    // Convert /dev/disk# to disk# for diskutil
    let disk_name = device_path.trim_start_matches("/dev/");

    crate::debug::log(&format!("Attempting: diskutil unmountDisk force {}", disk_name));

    // Try to unmount with a short timeout, but don't fail if it doesn't work
    let result = timeout(
        Duration::from_secs(5),  // Shorter timeout - 5 seconds only
        Command::new("diskutil")
            .arg("unmountDisk")
            .arg("force")
            .arg(disk_name)
            .output()
    ).await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            crate::debug::log(&format!("Disk unmounted: {}", stdout));
        }
        _ => {
            crate::debug::log("Unmount failed or timed out - proceeding with raw disk anyway");
            crate::debug::log("Note: Writing to /dev/rdisk# often works even when disk is mounted");
        }
    }

    tokio::time::sleep(Duration::from_millis(500)).await;
    Ok(())
}

#[cfg(target_os = "macos")]
async fn burn_image_macos(
    image_path: &Path,
    device_path: &str,
    image_size: u64,
    progress_tx: &UnboundedSender<BurnProgress>,
    cancel_token: &CancellationToken,
) -> Result<u64, String> {
    use std::io::{Read, Write};

    // Use rdisk for faster writes (raw disk)
    let raw_device_path = device_path.replace("/dev/disk", "/dev/rdisk");
    crate::debug::log(&format!("Using raw device: {}", raw_device_path));

    let bytes_written = tokio::task::spawn_blocking({
        let image_path = image_path.to_path_buf();
        let device_path = raw_device_path.clone();
        let progress_tx = progress_tx.clone();
        let cancel_token = cancel_token.clone();

        move || -> Result<u64, String> {
            crate::debug::log("Opening device using authopen (will prompt for authorization)...");

            // Use authopen to get privileged file descriptor with socketpair FD passing
            // This will show a native macOS authorization dialog
            let mut device = match crate::mac::authopen::auth_open_device(std::path::Path::new(&device_path)) {
                Ok(file) => file,
                Err(crate::mac::authopen::AuthOpenError::Cancelled) => {
                    crate::debug::log("User cancelled authorization");
                    return Err("Authorization cancelled by user".to_string());
                },
                Err(crate::mac::authopen::AuthOpenError::Failed(msg)) => {
                    crate::debug::log(&format!("Authorization failed: {}", msg));
                    return Err(msg); // msg already includes log path from authopen.rs
                },
                Err(crate::mac::authopen::AuthOpenError::SystemError(msg)) => {
                    crate::debug::log(&format!("System error during authorization: {}", msg));
                    let log_path = crate::debug::get_log_path();
                    return Err(format!("System error: {}\n\nDebug log: {:?}\nClick 'Copy Log to Clipboard' to share this error.", msg, log_path));
                },
            };

            crate::debug::log("Device opened successfully via authopen");

            // CRITICAL: Wipe the partition table FIRST to prevent macOS auto-remount
            // Similar to Windows implementation - this stops disk arbitration from
            // detecting and mounting partitions as we write them
            crate::debug::log("Wiping partition table to prevent auto-remount...");

            const WIPE_SIZE: usize = 1 * 1024 * 1024; // 1 MB
            let wipe_buffer = vec![0u8; WIPE_SIZE];

            device.write_all(&wipe_buffer)
                .map_err(|e| format!("Failed to wipe partition table: {}", e))?;

            // Sync the wipe (may fail on raw devices with ENOTTY, which is OK since O_SYNC is set)
            if let Err(e) = device.sync_all() {
                crate::debug::log(&format!("Note: sync_all after wipe failed (expected on raw devices): {}", e));
            } else {
                crate::debug::log("Partition table wipe synced successfully");
            }

            crate::debug::log("Partition table wiped, seeking back to start...");

            // Seek back to the beginning of the disk
            use std::io::Seek;
            device.seek(std::io::SeekFrom::Start(0))
                .map_err(|e| format!("Failed to seek to start after wipe: {}", e))?;

            crate::debug::log("Ready to write image");

            let mut image_reader = open_image_reader(&image_path)?;

            // macOS raw devices with F_NOCACHE require sector-aligned writes
            // Use similar buffering approach as Windows implementation
            const SECTOR_SIZE: usize = 512;
            let mut read_buffer = vec![0u8; CHUNK_SIZE];
            let sector_buffer_size = ((CHUNK_SIZE + SECTOR_SIZE - 1) / SECTOR_SIZE) * SECTOR_SIZE + SECTOR_SIZE;
            let mut sector_buffer = vec![0u8; sector_buffer_size];
            let mut sector_buffer_pos = 0usize;
            let mut total_written = 0u64;

            loop {
                if cancel_token.is_cancelled() {
                    crate::debug::log("Burn cancelled by user");
                    return Err("Burn cancelled".to_string());
                }

                // Read from image into read_buffer
                let bytes_read = image_reader.read(&mut read_buffer)
                    .map_err(|e| format!("Failed to read from image: {}", e))?;

                if bytes_read == 0 {
                    // EOF - handle any remaining partial sector
                    if sector_buffer_pos > 0 {
                        // Pad to sector boundary with zeros
                        let padded_size = ((sector_buffer_pos + SECTOR_SIZE - 1) / SECTOR_SIZE) * SECTOR_SIZE;
                        sector_buffer[sector_buffer_pos..padded_size].fill(0);

                        crate::debug::log(&format!("Writing final sector: {} bytes (padded from {})", padded_size, sector_buffer_pos));

                        device.write_all(&sector_buffer[..padded_size])
                            .map_err(|e| format!("Failed to write final sector at offset {}: {}", total_written, e))?;

                        total_written += sector_buffer_pos as u64; // Count only actual data, not padding
                    }
                    break;
                }

                // Append new data to sector buffer
                sector_buffer[sector_buffer_pos..sector_buffer_pos + bytes_read]
                    .copy_from_slice(&read_buffer[..bytes_read]);
                sector_buffer_pos += bytes_read;

                // Write all complete sectors
                let sectors_to_write = (sector_buffer_pos / SECTOR_SIZE) * SECTOR_SIZE;
                if sectors_to_write > 0 {
                    device.write_all(&sector_buffer[..sectors_to_write])
                        .map_err(|e| format!("Failed to write to device at offset {}: {}", total_written, e))?;

                    total_written += sectors_to_write as u64;
                    let _ = progress_tx.send(BurnProgress::Writing {
                        written: total_written,
                        total: image_size,
                    });

                    // Move remaining partial sector to start of buffer
                    let remaining = sector_buffer_pos - sectors_to_write;
                    if remaining > 0 {
                        sector_buffer.copy_within(sectors_to_write..sector_buffer_pos, 0);
                    }
                    sector_buffer_pos = remaining;
                }
            }

            // Sync to ensure all data is written (may fail on raw devices with ENOTTY, which is OK since O_SYNC is set)
            if let Err(e) = device.sync_all() {
                crate::debug::log(&format!("Note: final sync_all failed (expected on raw devices): {}", e));
                crate::debug::log("Data already synced via O_SYNC flag - this is safe");
            } else {
                crate::debug::log("Final sync completed successfully");
            }

            crate::debug::log(&format!("Write complete: {} bytes written", total_written));
            Ok(total_written)
        } // <-- Close the closure
    }) // <-- Close spawn_blocking
    .await
    .map_err(|e| format!("Write task failed: {}", e))??;

    Ok(bytes_written)
}
