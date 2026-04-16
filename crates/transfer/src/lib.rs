use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use walkdir::WalkDir;

pub const DEFAULT_CHUNK_SIZE: usize = 1024 * 1024;
pub const DEFAULT_MAX_RETRIES: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferManifest {
    pub transfer_id: String,
    pub root_name: String,
    pub chunk_size: usize,
    pub created_at: DateTime<Utc>,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub relative_path: String,
    pub size: u64,
    pub modified_unix: i64,
    pub file_hash: String,
    pub chunk_hashes: Vec<String>,
    pub resume_bitmap: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub current_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferReport {
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub files_total: usize,
    pub files_completed: usize,
}

impl ManifestEntry {
    pub fn chunk_count(&self) -> usize {
        self.chunk_hashes.len()
    }
}

pub fn build_manifest(path: impl AsRef<Path>, chunk_size: usize) -> Result<TransferManifest> {
    let chunk_size = if chunk_size == 0 {
        DEFAULT_CHUNK_SIZE
    } else {
        chunk_size
    };

    let root = path.as_ref().canonicalize().context("canonicalize path")?;
    let mut entries = Vec::new();

    if root.is_file() {
        let rel = root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string();
        entries.push(build_entry(&root, rel, chunk_size)?);
    } else {
        let mut files = WalkDir::new(&root)
            .into_iter()
            .filter_map(|res| res.ok())
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.into_path())
            .collect::<Vec<_>>();
        files.sort();
        for file in files {
            let rel = file
                .strip_prefix(&root)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            entries.push(build_entry(&file, rel, chunk_size)?);
        }
    }

    let root_name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("transfer-root")
        .to_string();

    Ok(TransferManifest {
        transfer_id: Uuid::new_v4().to_string(),
        root_name,
        chunk_size,
        created_at: Utc::now(),
        entries,
    })
}

pub fn verify_entry(
    root: impl AsRef<Path>,
    entry: &ManifestEntry,
    chunk_size: usize,
) -> Result<bool> {
    let full = root.as_ref().join(&entry.relative_path);
    let recalculated = build_entry(&full, entry.relative_path.clone(), chunk_size)?;
    Ok(
        recalculated.file_hash == entry.file_hash
            && recalculated.chunk_hashes == entry.chunk_hashes,
    )
}

pub fn save_manifest(path: impl AsRef<Path>, manifest: &TransferManifest) -> Result<()> {
    let body = serde_json::to_string_pretty(manifest).context("serialize manifest")?;
    fs::write(path, body).context("write manifest")?;
    Ok(())
}

pub fn load_manifest(path: impl AsRef<Path>) -> Result<TransferManifest> {
    let body = fs::read_to_string(path).context("read manifest")?;
    let manifest = serde_json::from_str(&body).context("parse manifest")?;
    Ok(manifest)
}

pub fn execute_local_transfer(
    source_path: impl AsRef<Path>,
    target_root: impl AsRef<Path>,
    manifest: &mut TransferManifest,
    checkpoint_path: Option<&Path>,
    max_retries: u32,
    mut on_progress: impl FnMut(ProgressEvent),
) -> Result<TransferReport> {
    let source_path = source_path.as_ref();
    let target_root = target_root.as_ref();
    fs::create_dir_all(target_root).context("create receive target root")?;

    let is_single_file = source_path.is_file()
        && manifest.entries.len() == 1
        && manifest.entries[0].relative_path == manifest.root_name;

    let bytes_total = total_size(manifest);
    let mut bytes_done = done_size(manifest);
    let mut files_completed = 0usize;

    on_progress(ProgressEvent {
        bytes_total,
        bytes_done,
        current_file: String::new(),
    });

    let chunk_size = manifest.chunk_size;
    for entry_idx in 0..manifest.entries.len() {
        let (relative_path, entry_size, chunk_hashes, entry_file_hash) = {
            let entry = &manifest.entries[entry_idx];
            (
                entry.relative_path.clone(),
                entry.size,
                entry.chunk_hashes.clone(),
                entry.file_hash.clone(),
            )
        };

        let src_file_path = if source_path.is_file() {
            source_path.to_path_buf()
        } else {
            source_path.join(&relative_path)
        };

        let dest_file_path = if is_single_file {
            target_root.join(&manifest.root_name)
        } else {
            target_root.join(&manifest.root_name).join(&relative_path)
        };

        if let Some(parent) = dest_file_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }

        let mut src = File::open(&src_file_path)
            .with_context(|| format!("open source file {}", src_file_path.display()))?;
        let mut dst = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&dest_file_path)
            .with_context(|| format!("open target file {}", dest_file_path.display()))?;

        dst.set_len(entry_size)
            .with_context(|| format!("set target size {}", dest_file_path.display()))?;

        for (chunk_idx, expected_chunk_hash) in chunk_hashes.iter().enumerate() {
            let expected_chunk_hash = expected_chunk_hash.clone();

            let offset = (chunk_idx * chunk_size) as u64;
            let remaining = entry_size.saturating_sub(offset);
            let read_len = remaining.min(chunk_size as u64) as usize;
            if read_len == 0 {
                continue;
            }

            let was_done = {
                let entry = &manifest.entries[entry_idx];
                is_chunk_done(entry, chunk_idx)
            };
            if was_done {
                if verify_chunk_at(&mut dst, offset, read_len, &expected_chunk_hash)? {
                    continue;
                }
                let entry = &mut manifest.entries[entry_idx];
                clear_chunk_done(entry, chunk_idx);
                bytes_done = bytes_done.saturating_sub(read_len as u64);
            }

            let mut last_error = None;
            for _attempt in 0..max_retries.max(1) {
                match copy_chunk(&mut src, &mut dst, offset, read_len, &expected_chunk_hash) {
                    Ok(()) => {
                        let entry = &mut manifest.entries[entry_idx];
                        mark_chunk_done(entry, chunk_idx);
                        bytes_done += read_len as u64;
                        if let Some(path) = checkpoint_path {
                            save_manifest(path, manifest)?;
                        }
                        on_progress(ProgressEvent {
                            bytes_total,
                            bytes_done,
                            current_file: relative_path.clone(),
                        });
                        last_error = None;
                        break;
                    }
                    Err(err) => last_error = Some(err),
                }
            }

            if let Some(err) = last_error {
                return Err(err).with_context(|| {
                    format!(
                        "copy chunk failed file={} chunk_idx={}",
                        relative_path, chunk_idx
                    )
                });
            }
        }

        let (file_hash, _) = hash_file(&dest_file_path, chunk_size)?;
        if file_hash != entry_file_hash {
            return Err(anyhow::anyhow!("file hash mismatch for {}", relative_path));
        }

        files_completed += 1;
    }

    if let Some(path) = checkpoint_path {
        save_manifest(path, manifest)?;
    }

    Ok(TransferReport {
        bytes_total,
        bytes_done,
        files_total: manifest.entries.len(),
        files_completed,
    })
}

pub fn mark_chunk_done(entry: &mut ManifestEntry, chunk_idx: usize) {
    set_bitmap(&mut entry.resume_bitmap, chunk_idx, true);
}

pub fn clear_chunk_done(entry: &mut ManifestEntry, chunk_idx: usize) {
    set_bitmap(&mut entry.resume_bitmap, chunk_idx, false);
}

pub fn is_chunk_done(entry: &ManifestEntry, chunk_idx: usize) -> bool {
    get_bitmap(&entry.resume_bitmap, chunk_idx)
}

fn copy_chunk(
    src: &mut File,
    dst: &mut File,
    offset: u64,
    read_len: usize,
    expected_chunk_hash: &str,
) -> Result<()> {
    let mut buf = vec![0_u8; read_len];

    src.seek(SeekFrom::Start(offset))
        .context("seek source offset")?;
    src.read_exact(&mut buf).context("read source chunk")?;

    let hash = blake3::hash(&buf).to_hex().to_string();
    if hash != expected_chunk_hash {
        return Err(anyhow::anyhow!("source chunk hash mismatch"));
    }

    dst.seek(SeekFrom::Start(offset))
        .context("seek target offset")?;
    dst.write_all(&buf).context("write target chunk")?;

    Ok(())
}

fn verify_chunk_at(
    file: &mut File,
    offset: u64,
    len: usize,
    expected_chunk_hash: &str,
) -> Result<bool> {
    let mut buf = vec![0_u8; len];
    file.seek(SeekFrom::Start(offset))
        .context("seek target for verify")?;
    file.read_exact(&mut buf).context("read target chunk")?;
    let hash = blake3::hash(&buf).to_hex().to_string();
    Ok(hash == expected_chunk_hash)
}

fn build_entry(path: &Path, relative_path: String, chunk_size: usize) -> Result<ManifestEntry> {
    let metadata = path.metadata().context("read metadata")?;
    let size = metadata.len();
    let modified = metadata
        .modified()
        .ok()
        .map(|m| DateTime::<Utc>::from(m).timestamp())
        .unwrap_or_default();

    let (file_hash, chunk_hashes) = hash_file(path, chunk_size)?;
    let resume_bitmap = vec![0_u8; chunk_hashes.len().div_ceil(8)];

    Ok(ManifestEntry {
        relative_path,
        size,
        modified_unix: modified,
        file_hash,
        chunk_hashes,
        resume_bitmap,
    })
}

fn hash_file(path: &Path, chunk_size: usize) -> Result<(String, Vec<String>)> {
    let mut file = File::open(path).with_context(|| format!("open file {}", path.display()))?;
    let mut file_hasher = blake3::Hasher::new();
    let mut chunk_hashes = Vec::new();
    let mut buf = vec![0_u8; chunk_size];

    loop {
        let n = file.read(&mut buf).context("read file chunk")?;
        if n == 0 {
            break;
        }
        let chunk = &buf[..n];
        file_hasher.update(chunk);
        chunk_hashes.push(blake3::hash(chunk).to_hex().to_string());
    }

    Ok((file_hasher.finalize().to_hex().to_string(), chunk_hashes))
}

fn set_bitmap(bitmap: &mut [u8], idx: usize, value: bool) {
    let byte_idx = idx / 8;
    let bit_idx = idx % 8;
    if let Some(byte) = bitmap.get_mut(byte_idx) {
        if value {
            *byte |= 1 << bit_idx;
        } else {
            *byte &= !(1 << bit_idx);
        }
    }
}

fn get_bitmap(bitmap: &[u8], idx: usize) -> bool {
    let byte_idx = idx / 8;
    let bit_idx = idx % 8;
    bitmap
        .get(byte_idx)
        .map(|b| (b & (1 << bit_idx)) != 0)
        .unwrap_or(false)
}

pub fn total_size(manifest: &TransferManifest) -> u64 {
    manifest.entries.iter().map(|entry| entry.size).sum()
}

pub fn done_size(manifest: &TransferManifest) -> u64 {
    let chunk_size = manifest.chunk_size;
    manifest
        .entries
        .iter()
        .map(|entry| {
            entry
                .chunk_hashes
                .iter()
                .enumerate()
                .filter_map(|(idx, _)| {
                    if is_chunk_done(entry, idx) {
                        let offset = (idx * chunk_size) as u64;
                        let remaining = entry.size.saturating_sub(offset);
                        Some(remaining.min(chunk_size as u64))
                    } else {
                        None
                    }
                })
                .sum::<u64>()
        })
        .sum()
}

pub fn relative_entry_paths(manifest: &TransferManifest) -> Vec<PathBuf> {
    manifest
        .entries
        .iter()
        .map(|entry| PathBuf::from(&entry.relative_path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_manifest_for_nested_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("data");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("a.txt"), b"hello").unwrap();
        fs::write(root.join("sub/b.txt"), b"world").unwrap();

        let manifest = build_manifest(&root, 4).unwrap();
        assert_eq!(manifest.entries.len(), 2);
        assert!(manifest
            .entries
            .iter()
            .any(|entry| entry.relative_path == "a.txt"));
        assert!(manifest
            .entries
            .iter()
            .any(|entry| entry.relative_path == "sub/b.txt"));
    }

    #[test]
    fn bitmap_updates_for_resume() {
        let mut entry = ManifestEntry {
            relative_path: "a.txt".to_string(),
            size: 100,
            modified_unix: 0,
            file_hash: "f".to_string(),
            chunk_hashes: vec!["1".into(), "2".into(), "3".into()],
            resume_bitmap: vec![0],
        };

        assert!(!is_chunk_done(&entry, 1));
        mark_chunk_done(&mut entry, 1);
        assert!(is_chunk_done(&entry, 1));
        clear_chunk_done(&mut entry, 1);
        assert!(!is_chunk_done(&entry, 1));
    }

    #[test]
    fn executes_and_resumes_transfer() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let out = tmp.path().join("out");
        fs::create_dir_all(src.join("nested")).unwrap();

        let payload = vec![7_u8; 1024 * 64 + 33];
        fs::write(src.join("nested/large.bin"), &payload).unwrap();
        fs::write(src.join("a.txt"), b"hello").unwrap();

        let mut manifest = build_manifest(&src, 4096).unwrap();
        let checkpoint = tmp.path().join("checkpoint.json");

        let report = execute_local_transfer(
            &src,
            &out,
            &mut manifest,
            Some(&checkpoint),
            DEFAULT_MAX_RETRIES,
            |_ev| {},
        )
        .unwrap();
        assert_eq!(report.bytes_done, report.bytes_total);

        let mut loaded = load_manifest(&checkpoint).unwrap();
        let report2 = execute_local_transfer(
            &src,
            &out,
            &mut loaded,
            Some(&checkpoint),
            DEFAULT_MAX_RETRIES,
            |_ev| {},
        )
        .unwrap();
        assert_eq!(report2.bytes_done, report2.bytes_total);

        let copied = fs::read(out.join("src/nested/large.bin")).unwrap();
        assert_eq!(copied, payload);
    }
}
