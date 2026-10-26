// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

// Single-slot devices: after the base image is burned, add a FAT32 SPRUCEOS
// partition behind the rootfs and copy spruce into it, all through the raw
// device handle the burn already holds. The partition sits where the image's
// firstboot would put it (rootfs start + rootfs size, 1 MiB aligned); firstboot
// then grows the rootfs up to it and leaves it alone.

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::burn::BurnProgress;

const SECTOR: u64 = 512;
const ALIGN_SECTORS: u64 = 2048;
const LABEL: [u8; 11] = *b"SPRUCEOS   ";
const FAT32_LBA_TYPE: u8 = 0x0C;
const GPT_PROTECTIVE_TYPE: u8 = 0xEE;
// EBD0A0A2-B9E5-4433-87C0-68B6B72699C7, Microsoft basic data, as stored on disk
const GPT_BASIC_DATA: [u8; 16] = [
    0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7,
];

pub struct SingleCard {
    pub spruce_dir: PathBuf,
    pub rootfs_bytes: u64,
}

fn read_at<D: Read + Seek>(dev: &mut D, offset: u64, len: usize) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    dev.seek(SeekFrom::Start(offset))?;
    dev.read_exact(&mut buf)?;
    Ok(buf)
}

fn write_at<D: Write + Seek>(dev: &mut D, offset: u64, data: &[u8]) -> io::Result<()> {
    dev.seek(SeekFrom::Start(offset))?;
    dev.write_all(data)
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}

fn align_up(sector: u64) -> u64 {
    (sector + ALIGN_SECTORS - 1) / ALIGN_SECTORS * ALIGN_SECTORS
}

/// The burned partition table and where the new partition goes.
enum Table {
    Mbr { mbr: Vec<u8>, slot: usize },
    Gpt { mbr: Vec<u8>, header: Vec<u8>, entries: Vec<u8>, slot: usize },
}

struct Plan {
    table: Table,
    start: u64,
    end: u64, // exclusive
}

fn plan<D: Read + Seek>(dev: &mut D, disk_sectors: u64, rootfs_bytes: u64) -> Result<Plan, String> {
    let read_err = |e: io::Error| format!("Failed to read the partition table: {}", e);
    let mbr = read_at(dev, 0, SECTOR as usize).map_err(read_err)?;
    if mbr[510] != 0x55 || mbr[511] != 0xAA {
        return Err("The burned image has no partition table".to_string());
    }

    if mbr[446 + 4] != GPT_PROTECTIVE_TYPE {
        let mut last: Option<(u64, u64)> = None;
        let mut slot = None;
        for i in 0..4 {
            let e = &mbr[446 + i * 16..446 + (i + 1) * 16];
            let (start, size) = (u32_at(e, 8) as u64, u32_at(e, 12) as u64);
            if e[4] == 0 || size == 0 {
                slot.get_or_insert(i);
            } else if last.map_or(true, |(s, z)| start + size > s + z) {
                last = Some((start, size));
            }
        }
        let (rootfs_start, _) = last.ok_or("The burned image has no partitions")?;
        let slot = slot.ok_or("The burned image has no free partition slot")?;
        let start = align_up(rootfs_start + rootfs_bytes / SECTOR);
        let end = disk_sectors.min(u32::MAX as u64);
        return Ok(Plan { table: Table::Mbr { mbr, slot }, start, end });
    }

    let header = read_at(dev, SECTOR, SECTOR as usize).map_err(read_err)?;
    if &header[0..8] != b"EFI PART" {
        return Err("The burned image has a protective MBR but no GPT header".to_string());
    }
    let entries_lba = u64_at(&header, 72);
    let count = u32_at(&header, 80) as usize;
    let size = u32_at(&header, 84) as usize;
    let entries_len = ((count * size) as u64 + SECTOR - 1) / SECTOR * SECTOR;
    let entries = read_at(dev, entries_lba * SECTOR, entries_len as usize).map_err(read_err)?;

    let mut last: Option<(u64, u64)> = None;
    let mut slot = None;
    for i in 0..count {
        let e = &entries[i * size..(i + 1) * size];
        if e[0..16].iter().all(|&b| b == 0) {
            slot.get_or_insert(i);
        } else {
            let (first, last_lba) = (u64_at(e, 32), u64_at(e, 40));
            if last.map_or(true, |(_, l)| last_lba > l) {
                last = Some((first, last_lba));
            }
        }
    }
    let (rootfs_start, _) = last.ok_or("The burned image has no partitions")?;
    let slot = slot.ok_or("The burned image has no free partition slot")?;
    let start = align_up(rootfs_start + rootfs_bytes / SECTOR);
    // Room for the backup entries and header at the end of the card.
    let end = disk_sectors - entries_len / SECTOR - 1;
    Ok(Plan { table: Table::Gpt { mbr, header, entries, slot }, start, end })
}

fn random_guid() -> [u8; 16] {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut g = [0u8; 16];
    for half in g.chunks_mut(8) {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
        half.copy_from_slice(&h.finish().to_le_bytes());
    }
    g[7] = (g[7] & 0x0F) | 0x40;
    g[8] = (g[8] & 0x3F) | 0x80;
    g
}

fn gpt_header_crc(header: &mut [u8]) {
    let len = u32_at(header, 12) as usize;
    header[16..20].fill(0);
    let crc = crc32fast::hash(&header[..len]);
    header[16..20].copy_from_slice(&crc.to_le_bytes());
}

fn write_table<D: Write + Seek>(dev: &mut D, plan: Plan, disk_sectors: u64) -> io::Result<()> {
    match plan.table {
        Table::Mbr { mut mbr, slot } => {
            let e = &mut mbr[446 + slot * 16..446 + (slot + 1) * 16];
            e.fill(0);
            e[1..4].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
            e[4] = FAT32_LBA_TYPE;
            e[5..8].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
            e[8..12].copy_from_slice(&(plan.start as u32).to_le_bytes());
            e[12..16].copy_from_slice(&((plan.end - plan.start) as u32).to_le_bytes());
            write_at(dev, 0, &mbr)
        }
        Table::Gpt { mut mbr, mut header, mut entries, slot } => {
            let size = u32_at(&header, 84) as usize;
            let e = &mut entries[slot * size..(slot + 1) * size];
            e.fill(0);
            e[0..16].copy_from_slice(&GPT_BASIC_DATA);
            e[16..32].copy_from_slice(&random_guid());
            e[32..40].copy_from_slice(&plan.start.to_le_bytes());
            e[40..48].copy_from_slice(&(plan.end - 1).to_le_bytes());
            for (i, c) in "SPRUCEOS".encode_utf16().enumerate() {
                e[56 + i * 2..58 + i * 2].copy_from_slice(&c.to_le_bytes());
            }

            let count = u32_at(&header, 80) as usize;
            let entries_crc = crc32fast::hash(&entries[..count * size]);
            let backup_lba = disk_sectors - 1;
            let backup_entries_lba = plan.end;

            header[32..40].copy_from_slice(&backup_lba.to_le_bytes());
            header[48..56].copy_from_slice(&(plan.end - 1).to_le_bytes());
            header[88..92].copy_from_slice(&entries_crc.to_le_bytes());
            let mut backup = header.clone();
            gpt_header_crc(&mut header);

            backup[24..32].copy_from_slice(&backup_lba.to_le_bytes());
            backup[32..40].copy_from_slice(&1u64.to_le_bytes());
            backup[72..80].copy_from_slice(&backup_entries_lba.to_le_bytes());
            gpt_header_crc(&mut backup);

            let protective = (disk_sectors - 1).min(u32::MAX as u64) as u32;
            mbr[446 + 12..446 + 16].copy_from_slice(&protective.to_le_bytes());

            write_at(dev, backup_entries_lba * SECTOR, &entries)?;
            write_at(dev, backup_lba * SECTOR, &backup)?;
            write_at(dev, u64_at(&header, 72) * SECTOR, &entries)?;
            write_at(dev, SECTOR, &header)?;
            write_at(dev, 0, &mbr)
        }
    }
}

const CHUNK: u64 = 1 << 20;
const MAX_CHUNKS: usize = 64;

struct Chunk {
    data: Vec<u8>,
    dirty: bool,
    used: u64,
}

/// A window onto one partition of the raw device. Raw devices on Windows and
/// macOS only take whole-sector I/O, and fatfs writes a few bytes at a time,
/// so every access goes through a write-back cache of 1 MiB chunks.
struct PartitionIo<'a, D: Read + Write + Seek> {
    dev: &'a mut D,
    base: u64,
    len: u64,
    pos: u64,
    chunks: HashMap<u64, Chunk>,
    tick: u64,
}

impl<'a, D: Read + Write + Seek> PartitionIo<'a, D> {
    fn new(dev: &'a mut D, base: u64, len: u64) -> Self {
        Self { dev, base, len, pos: 0, chunks: HashMap::new(), tick: 0 }
    }

    fn write_back(dev: &mut D, base: u64, index: u64, chunk: &mut Chunk) -> io::Result<()> {
        if chunk.dirty {
            write_at(dev, base + index * CHUNK, &chunk.data)?;
            chunk.dirty = false;
        }
        Ok(())
    }

    fn chunk(&mut self, index: u64) -> io::Result<&mut Chunk> {
        self.tick += 1;
        if !self.chunks.contains_key(&index) {
            if self.chunks.len() >= MAX_CHUNKS {
                let oldest = *self.chunks.iter().min_by_key(|(_, c)| c.used).unwrap().0;
                let mut evicted = self.chunks.remove(&oldest).unwrap();
                Self::write_back(self.dev, self.base, oldest, &mut evicted)?;
            }
            let len = CHUNK.min(self.len - index * CHUNK) as usize;
            let data = read_at(self.dev, self.base + index * CHUNK, len)?;
            self.chunks.insert(index, Chunk { data, dirty: false, used: 0 });
        }
        let chunk = self.chunks.get_mut(&index).unwrap();
        chunk.used = self.tick;
        Ok(chunk)
    }

    fn sync(&mut self) -> io::Result<()> {
        for (index, chunk) in self.chunks.iter_mut() {
            Self::write_back(self.dev, self.base, *index, chunk)?;
        }
        self.dev.flush()
    }
}

impl<D: Read + Write + Seek> Read for PartitionIo<'_, D> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let (index, offset) = (self.pos / CHUNK, (self.pos % CHUNK) as usize);
        let chunk = self.chunk(index)?;
        let n = buf.len().min(chunk.data.len() - offset);
        buf[..n].copy_from_slice(&chunk.data[offset..offset + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl<D: Read + Write + Seek> Write for PartitionIo<'_, D> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.pos >= self.len {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "write past the end of the partition"));
        }
        if buf.is_empty() {
            return Ok(0);
        }
        let (index, offset) = (self.pos / CHUNK, (self.pos % CHUNK) as usize);
        let chunk = self.chunk(index)?;
        let n = buf.len().min(chunk.data.len() - offset);
        chunk.data[offset..offset + n].copy_from_slice(&buf[..n]);
        chunk.dirty = true;
        self.pos += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<D: Read + Write + Seek> Seek for PartitionIo<'_, D> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(d) => self.len as i128 + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if target < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before the start of the partition"));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

fn collect(dir: &Path, dirs: &mut Vec<PathBuf>, files: &mut Vec<(PathBuf, u64)>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            dirs.push(path.clone());
            collect(&path, dirs, files)?;
        } else {
            let len = std::fs::metadata(&path)?.len();
            files.push((path, len));
        }
    }
    Ok(())
}

fn fat_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Bytes the SPRUCEOS partition needs for this spruce tree, with headroom for
/// cluster slack and the FATs.
pub fn space_needed(spruce_dir: &Path) -> io::Result<u64> {
    let (mut dirs, mut files) = (Vec::new(), Vec::new());
    collect(spruce_dir, &mut dirs, &mut files)?;
    let bytes: u64 = files.iter().map(|(_, len)| len).sum();
    let entries = (dirs.len() + files.len()) as u64;
    Ok(bytes + bytes / 10 + entries * 32 * 1024 + 256 * 1024 * 1024)
}

/// Runs right after the image is written, on the same open device.
pub fn install<D: Read + Write + Seek>(
    dev: &mut D,
    disk_bytes: u64,
    card: &SingleCard,
    progress_tx: &UnboundedSender<BurnProgress>,
    cancel_token: &CancellationToken,
) -> Result<(), String> {
    crate::debug::log_section("Single-card spruce partition");
    let disk_sectors = disk_bytes / SECTOR;
    let plan = plan(dev, disk_sectors, card.rootfs_bytes)?;
    let part_bytes = plan.end.saturating_sub(plan.start) * SECTOR;
    crate::debug::log(&format!(
        "Disk {} sectors; SPRUCEOS at sector {}..{} ({} MB)",
        disk_sectors, plan.start, plan.end, part_bytes / 1_048_576
    ));

    let needed = space_needed(&card.spruce_dir).map_err(|e| format!("Failed to read the spruce files: {}", e))?;
    if part_bytes < needed {
        return Err(format!(
            "The card is too small for dArkMoss and spruce together: {} MB left for spruce, {} MB needed",
            part_bytes / 1_048_576,
            needed / 1_048_576
        ));
    }

    let _ = progress_tx.send(BurnProgress::Formatting);
    let mut part = PartitionIo::new(dev, plan.start * SECTOR, part_bytes);
    fatfs::format_volume(
        &mut part,
        fatfs::FormatVolumeOptions::new().fat_type(fatfs::FatType::Fat32).volume_label(LABEL),
    )
    .map_err(|e| format!("Failed to format SPRUCEOS: {}", e))?;
    part.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;

    let (mut dirs, mut files) = (Vec::new(), Vec::new());
    collect(&card.spruce_dir, &mut dirs, &mut files).map_err(|e| format!("Failed to read the spruce files: {}", e))?;
    let total: u64 = files.iter().map(|(_, len)| len).sum();
    crate::debug::log(&format!("Copying {} files, {} dirs, {} bytes", files.len(), dirs.len(), total));

    {
        let fs = fatfs::FileSystem::new(&mut part, fatfs::FsOptions::new())
            .map_err(|e| format!("Failed to open SPRUCEOS: {}", e))?;
        let root = fs.root_dir();
        for dir in &dirs {
            let name = fat_path(&card.spruce_dir, dir);
            root.create_dir(&name).map_err(|e| format!("Failed to create {}: {}", name, e))?;
        }

        let mut copied = 0u64;
        let mut buf = vec![0u8; CHUNK as usize];
        for (path, _) in &files {
            if cancel_token.is_cancelled() {
                return Err("Burn cancelled".to_string());
            }
            let name = fat_path(&card.spruce_dir, path);
            let mut src = std::fs::File::open(path).map_err(|e| format!("Failed to open {:?}: {}", path, e))?;
            let mut dst = root.create_file(&name).map_err(|e| format!("Failed to create {}: {}", name, e))?;
            loop {
                let n = src.read(&mut buf).map_err(|e| format!("Failed to read {:?}: {}", path, e))?;
                if n == 0 {
                    break;
                }
                dst.write_all(&buf[..n]).map_err(|e| format!("Failed to write {}: {}", name, e))?;
                copied += n as u64;
            }
            let _ = progress_tx.send(BurnProgress::Copying { copied, total });
        }
        fs.unmount().map_err(|e| format!("Failed to finish SPRUCEOS: {}", e))?;
    }
    part.sync().map_err(|e| format!("Failed to write SPRUCEOS: {}", e))?;

    // Last, so nothing sees the partition before its contents are on the card.
    write_table(dev, plan, disk_sectors).map_err(|e| format!("Failed to write the partition table: {}", e))?;
    dev.flush().map_err(|e| e.to_string())?;
    crate::debug::log("SPRUCEOS partition written");
    Ok(())
}
