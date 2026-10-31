//! Where the bytes of volumes and tables live: a bucket (Cloudflare R2 or any S3), or a folder on a POSIX path.
//!
//! The bucket is the product's home for data, so a remote machine can read it. The server alone holds the bucket's credentials;
//! a machine that runs a query is given short-lived signed addresses for exactly the files it needs ([`Store::sign_get`]).

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use futures::{StreamExt, TryStreamExt, stream::BoxStream};
use object_store::{
    ObjectStore, WriteMultipart,
    aws::{AmazonS3, AmazonS3Builder},
    path::Path as Key,
    signer::Signer,
};
use tokio_util::io::ReaderStream;

use crate::files::{Entry, FileError, Upload as FolderUpload, Volumes, check_volume};

/// Part size of a bucket upload: the most memory one upload holds.
const PART: usize = 8 * 1024 * 1024;

/// How to reach a bucket. The secrets are values the caller read from mounted files.
pub struct Bucket {
    pub endpoint: String,
    pub name: String,
    pub key_id: String,
    pub secret: String,
}

#[derive(Clone)]
pub enum Store {
    Folder(Volumes),
    Bucket(Arc<AmazonS3>),
}

pub type Reading = BoxStream<'static, std::io::Result<Bytes>>;

impl Store {
    #[must_use]
    pub fn folder(root: impl Into<std::path::PathBuf>) -> Self {
        Self::Folder(Volumes::new(root))
    }

    /// # Errors
    /// The bucket settings are not usable.
    pub fn bucket(b: &Bucket) -> Result<Self, FileError> {
        let s3 = AmazonS3Builder::new()
            .with_endpoint(&b.endpoint)
            .with_bucket_name(&b.name)
            .with_access_key_id(&b.key_id)
            .with_secret_access_key(&b.secret)
            .with_region("auto")
            .build()
            .map_err(|e| FileError::Io(std::io::Error::other(e.to_string())))?;
        Ok(Self::Bucket(Arc::new(s3)))
    }

    /// Whether machines other than this server can be handed signed addresses.
    #[must_use]
    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Bucket(_))
    }

    fn key(volume: &str, rel: &str) -> Result<Key, FileError> {
        check_volume(volume)?;
        let parts = Volumes::components(rel)?;
        Ok(Key::from(format!("{volume}/{}", parts.join("/"))))
    }

    fn prefix(volume: &str) -> Result<Key, FileError> {
        check_volume(volume)?;
        Ok(Key::from(volume))
    }

    /// Starts an upload. Nothing is visible at `rel` until `commit`.
    pub async fn begin_put(&self, volume: &str, rel: &str, max: u64) -> Result<Upload, FileError> {
        match self {
            Self::Folder(v) => Ok(Upload::Folder(v.begin_put(volume, rel, max).await?)),
            Self::Bucket(s3) => {
                let up = s3
                    .put_multipart(&Self::key(volume, rel)?)
                    .await
                    .map_err(io)?;
                Ok(Upload::Bucket(BucketUpload {
                    write: Some(WriteMultipart::new_with_chunk_size(up, PART)),
                    size: 0,
                    max,
                }))
            }
        }
    }

    /// Opens a file for reading, with its size.
    pub async fn open(&self, volume: &str, rel: &str) -> Result<(Reading, u64), FileError> {
        match self {
            Self::Folder(v) => {
                let (file, size) = v.open(volume, rel).await?;
                Ok((ReaderStream::new(file).boxed(), size))
            }
            Self::Bucket(s3) => {
                let got = s3.get(&Self::key(volume, rel)?).await.map_err(gone)?;
                let size = got.meta.size;
                Ok((
                    got.into_stream().map_err(std::io::Error::other).boxed(),
                    size,
                ))
            }
        }
    }

    pub async fn delete(&self, volume: &str, rel: &str) -> Result<(), FileError> {
        match self {
            Self::Folder(v) => v.delete(volume, rel).await,
            Self::Bucket(s3) => {
                let key = Self::key(volume, rel)?;
                // A bucket deletes quietly what is not there; a caller should be told.
                s3.head(&key).await.map_err(gone)?;
                s3.delete(&key).await.map_err(io)
            }
        }
    }

    /// Every file in the volume, sorted by path.
    pub async fn list(&self, volume: &str) -> Result<Vec<Entry>, FileError> {
        match self {
            Self::Folder(v) => v.list(volume).await,
            Self::Bucket(s3) => {
                let prefix = Self::prefix(volume)?;
                let cut = volume.len() + 1;
                let mut out: Vec<Entry> = s3
                    .list(Some(&prefix))
                    .map_ok(|m| Entry {
                        path: m.location.as_ref()[cut..].to_string(),
                        size: m.size,
                    })
                    .try_collect()
                    .await
                    .map_err(io)?;
                out.sort_by(|a, b| a.path.cmp(&b.path));
                Ok(out)
            }
        }
    }

    /// Removes the whole volume. Removing one that is already gone is fine: a retried task must be able to run again.
    pub async fn remove_volume(&self, volume: &str) -> Result<(), FileError> {
        match self {
            Self::Folder(v) => v.remove_volume(volume).await,
            Self::Bucket(s3) => {
                let prefix = Self::prefix(volume)?;
                let keys = s3.list(Some(&prefix)).map_ok(|m| m.location).boxed();
                s3.delete_stream(keys)
                    .try_collect::<Vec<_>>()
                    .await
                    .map_err(io)?;
                Ok(())
            }
        }
    }

    /// Whether any bytes of the volume are left.
    pub async fn exists(&self, volume: &str) -> Result<bool, FileError> {
        match self {
            Self::Folder(v) => v.exists(volume).await,
            Self::Bucket(s3) => {
                let prefix = Self::prefix(volume)?;
                let first = s3.list(Some(&prefix)).next().await;
                first.transpose().map(|m| m.is_some()).map_err(io)
            }
        }
    }

    /// A time-limited address from which whoever holds it can read one file, and nothing else.
    ///
    /// # Errors
    /// The store is a folder (only a bucket can be reached by other machines), or the path is not allowed.
    pub async fn sign_get(
        &self,
        volume: &str,
        rel: &str,
        valid: Duration,
    ) -> Result<String, FileError> {
        self.sign(http::Method::GET, volume, rel, valid).await
    }

    /// The same, for writing one file.
    ///
    /// # Errors
    /// As [`Store::sign_get`].
    pub async fn sign_put(
        &self,
        volume: &str,
        rel: &str,
        valid: Duration,
    ) -> Result<String, FileError> {
        self.sign(http::Method::PUT, volume, rel, valid).await
    }

    async fn sign(
        &self,
        method: http::Method,
        volume: &str,
        rel: &str,
        valid: Duration,
    ) -> Result<String, FileError> {
        let Self::Bucket(s3) = self else {
            return Err(FileError::BadPath("this store is not a bucket"));
        };
        let url = s3
            .signed_url(method, &Self::key(volume, rel)?, valid)
            .await
            .map_err(io)?;
        Ok(url.into())
    }
}

/// A file being written. Dropping it without `commit` abandons it.
pub enum Upload {
    Folder(FolderUpload),
    Bucket(BucketUpload),
}

pub struct BucketUpload {
    write: Option<WriteMultipart>,
    size: u64,
    max: u64,
}

impl Upload {
    pub async fn write(&mut self, chunk: &[u8]) -> Result<(), FileError> {
        match self {
            Self::Folder(u) => u.write(chunk).await,
            Self::Bucket(u) => {
                u.size += chunk.len() as u64;
                if u.size > u.max {
                    return Err(FileError::TooLarge);
                }
                let w = u.write.as_mut().ok_or(FileError::NotFound)?;
                // Waits while too many parts are in flight, so a fast client cannot make the server hold the whole file.
                w.wait_for_capacity(2).await.map_err(io)?;
                w.write(chunk);
                Ok(())
            }
        }
    }

    /// Makes the file visible, replacing any earlier one atomically. Returns its size.
    pub async fn commit(self) -> Result<u64, FileError> {
        match self {
            Self::Folder(u) => u.commit().await,
            Self::Bucket(mut u) => {
                let w = u.write.take().ok_or(FileError::NotFound)?;
                w.finish().await.map_err(io)?;
                Ok(u.size)
            }
        }
    }
}

impl Drop for BucketUpload {
    fn drop(&mut self) {
        if let Some(w) = self.write.take() {
            // Frees the parts already sent; nothing is visible either way.
            tokio::spawn(async move {
                let _ = w.abort().await;
            });
        }
    }
}

fn io(e: object_store::Error) -> FileError {
    FileError::Io(std::io::Error::other(e.to_string()))
}

fn gone(e: object_store::Error) -> FileError {
    match e {
        object_store::Error::NotFound { .. } => FileError::NotFound,
        e => io(e),
    }
}
