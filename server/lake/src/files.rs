//! Volumes: a folder per volume on a POSIX path (a local disk, or any folder the infrastructure has mounted).
//!
//! Everything here is about staying inside the volume's own folder: a path from a caller never escapes it, and a
//! symlink is never followed, so a link planted inside a volume cannot be used to read or write something else.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use tokio::{fs, io::AsyncWriteExt};

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("invalid path: {0}")]
    BadPath(&'static str),
    #[error("not found")]
    NotFound,
    #[error("file too large")]
    TooLarge,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// One file inside a volume.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub size: u64,
}

#[derive(Clone)]
pub struct Volumes {
    root: PathBuf,
}

/// Uploads are written to a hidden temporary file and renamed into place, so a reader never sees half a file.
const TEMP: &str = ".upload-";
static COUNTER: AtomicU64 = AtomicU64::new(0);

impl Volumes {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The folder of one volume. Volume ids are ours (UUIDs), but they are checked anyway: this is the one place that
    /// turns a name into a path.
    fn dir(&self, volume: &str) -> Result<PathBuf, FileError> {
        let ok = !volume.is_empty()
            && volume.len() <= 64
            && volume
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !ok {
            return Err(FileError::BadPath("volume id"));
        }
        Ok(self.root.join(volume))
    }

    /// Splits a caller's path into safe components. No empty parts, no `.` or `..`, no backslashes or control characters.
    fn components(rel: &str) -> Result<Vec<&str>, FileError> {
        if rel.is_empty() || rel.len() > 1024 {
            return Err(FileError::BadPath("length"));
        }
        let parts: Vec<&str> = rel.split('/').collect();
        for p in &parts {
            let bad = p.is_empty()
                || *p == "."
                || *p == ".."
                || p.len() > 255
                || p.starts_with(TEMP)
                || p.chars().any(|c| c == '\\' || c.is_control());
            if bad {
                return Err(FileError::BadPath("component"));
            }
        }
        Ok(parts)
    }

    /// The volume folder plus the components, refusing if any part that already exists is a symlink.
    async fn resolve(&self, volume: &str, rel: &str) -> Result<PathBuf, FileError> {
        let parts = Self::components(rel)?;
        let mut path = self.dir(volume)?;
        no_link(&path).await?;
        for part in parts {
            path.push(part);
            no_link(&path).await?;
        }
        Ok(path)
    }

    /// Starts an upload. Nothing is visible at `rel` until `commit`.
    pub async fn begin_put(&self, volume: &str, rel: &str, max: u64) -> Result<Upload, FileError> {
        let target = self.resolve(volume, rel).await?;
        let parent = target.parent().ok_or(FileError::BadPath("component"))?;
        fs::create_dir_all(parent).await?;
        // The folders may have just been created; check again that none of them is a link.
        let _ = self.resolve(volume, rel).await?;
        if fs::symlink_metadata(&target)
            .await
            .is_ok_and(|m| m.is_dir())
        {
            return Err(FileError::BadPath("is a folder"));
        }
        let temp = parent.join(format!(
            "{TEMP}{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .await?;
        Ok(Upload {
            file: Some(file),
            temp,
            target,
            size: 0,
            max,
        })
    }

    /// Opens a file for reading, with its size.
    pub async fn open(&self, volume: &str, rel: &str) -> Result<(fs::File, u64), FileError> {
        let path = self.resolve(volume, rel).await?;
        let meta = fs::symlink_metadata(&path).await.map_err(not_found)?;
        if !meta.is_file() {
            return Err(FileError::NotFound);
        }
        let file = fs::File::open(&path).await.map_err(not_found)?;
        Ok((file, meta.len()))
    }

    pub async fn delete(&self, volume: &str, rel: &str) -> Result<(), FileError> {
        let path = self.resolve(volume, rel).await?;
        let meta = fs::symlink_metadata(&path).await.map_err(not_found)?;
        if !meta.is_file() {
            return Err(FileError::NotFound);
        }
        fs::remove_file(&path).await?;
        Ok(())
    }

    /// Every file in the volume, sorted by path. Links and unfinished uploads are not listed.
    pub async fn list(&self, volume: &str) -> Result<Vec<Entry>, FileError> {
        let base = self.dir(volume)?;
        let mut out = Vec::new();
        if fs::symlink_metadata(&base).await.is_err() {
            return Ok(out);
        }
        no_link(&base).await?;
        let mut stack = vec![(base.clone(), String::new())];
        while let Some((dir, prefix)) = stack.pop() {
            let mut rd = fs::read_dir(&dir).await?;
            while let Some(e) = rd.next_entry().await? {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with(TEMP) {
                    continue;
                }
                let meta = fs::symlink_metadata(e.path()).await?;
                let rel = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                if meta.is_dir() {
                    stack.push((e.path(), rel));
                } else if meta.is_file() {
                    out.push(Entry {
                        path: rel,
                        size: meta.len(),
                    });
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Removes the whole volume. Removing one that is already gone is fine: a retried task must be able to run again.
    pub async fn remove_volume(&self, volume: &str) -> Result<(), FileError> {
        let base = self.dir(volume)?;
        match fs::symlink_metadata(&base).await {
            Err(_) => Ok(()),
            Ok(m) if m.file_type().is_symlink() => {
                fs::remove_file(&base).await?;
                Ok(())
            }
            Ok(_) => {
                fs::remove_dir_all(&base).await?;
                Ok(())
            }
        }
    }

    pub async fn exists(&self, volume: &str) -> Result<bool, FileError> {
        Ok(fs::symlink_metadata(self.dir(volume)?).await.is_ok())
    }
}

/// A file being written. Dropping it without `commit` removes the temporary file.
pub struct Upload {
    file: Option<fs::File>,
    temp: PathBuf,
    target: PathBuf,
    size: u64,
    max: u64,
}

impl Upload {
    pub async fn write(&mut self, chunk: &[u8]) -> Result<(), FileError> {
        self.size += chunk.len() as u64;
        if self.size > self.max {
            return Err(FileError::TooLarge);
        }
        match self.file.as_mut() {
            Some(f) => Ok(f.write_all(chunk).await?),
            None => Err(FileError::NotFound),
        }
    }

    /// Makes the file visible, replacing any earlier one atomically. Returns its size.
    pub async fn commit(mut self) -> Result<u64, FileError> {
        if let Some(mut f) = self.file.take() {
            f.flush().await?;
        }
        fs::rename(&self.temp, &self.target).await?;
        Ok(self.size)
    }
}

impl Drop for Upload {
    fn drop(&mut self) {
        if self.file.take().is_some() {
            let _ = std::fs::remove_file(&self.temp);
        }
    }
}

async fn no_link(path: &Path) -> Result<(), FileError> {
    match fs::symlink_metadata(path).await {
        Ok(m) if m.file_type().is_symlink() => Err(FileError::BadPath("symlink")),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn not_found(e: std::io::Error) -> FileError {
    if e.kind() == std::io::ErrorKind::NotFound {
        FileError::NotFound
    } else {
        e.into()
    }
}
