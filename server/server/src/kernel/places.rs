//! Where a folder's bytes are. A securable's folder is a path in a store. Most are in the installation's own store; the folders of a
//! catalog put on a customer's storage location begin with `loc-<location id>/`, and everything below that catalog inherits it. A
//! [`Places`] looks at that first segment and sends the call to the right store, so code that reads and writes folders does not
//! know where the folder is.
//!
//! A customer's store is built from the location's credential, which is read from the vault at the version the database pins. The
//! built store is kept for at most an hour, and is dropped as soon as the credential's generation changes or the location goes
//! away, so a rotated or deleted credential is not used again by this server.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use loamz_lake::{
    files::{Entry, FileError},
    grants::Issuer,
    location::{LocationHandle, Place},
    objects::{Reading, Store, Upload},
};

/// The longest a built store is kept.
const LIFE: Duration = Duration::from_hours(1);
/// What starts the first segment of a folder that is on a customer's location.
const MARK: &str = "loc-";

/// What opening a location gives: the built store, and the folder of the bucket the location is rooted at.
pub struct Opened {
    pub handle: LocationHandle,
    pub prefix: String,
}

/// Where the locations are known. The storage feature gives one; the kernel only asks.
#[async_trait]
pub trait LocationSource: Send + Sync {
    /// The generation of the location's credential now, or None if the location (or its credential) no longer exists.
    async fn generation(&self, location: &str) -> Result<Option<i64>, String>;

    /// Builds the location's store from its credential. The message never carries a secret.
    async fn open(&self, location: &str) -> Result<Opened, String>;
}

struct Kept {
    at: Instant,
    generation: i64,
    handle: Arc<LocationHandle>,
    prefix: String,
}

/// Where one folder is, and what is needed to reach it.
pub struct Spot {
    pub store: Store,
    /// Issues credentials for folders of this store (None where the store cannot).
    pub issuer: Option<Arc<dyn Issuer>>,
    /// The folder as the store knows it: without the location marker.
    pub rest: String,
    /// The folder's real prefix in the bucket (the location's folder, then `rest`): what a door's grant is written in.
    pub key: String,
    /// The location it is on, or None for the installation's own store.
    pub location: Option<String>,
    /// Endpoint, region and bucket, for a machine that is given access to it. None for a store that is not a bucket.
    pub place: Option<Place>,
}

pub struct Places {
    own: Store,
    own_issuer: Option<Arc<dyn Issuer>>,
    own_place: Option<Place>,
    source: OnceLock<Arc<dyn LocationSource>>,
    kept: Mutex<HashMap<String, Kept>>,
}

/// A location's id, and the rest of the folder, if the folder is on a location.
fn split(folder: &str) -> (Option<&str>, &str) {
    let (first, rest) = folder.split_once('/').unwrap_or((folder, ""));
    match first.strip_prefix(MARK) {
        Some(id) if !id.is_empty() => (Some(id), rest),
        _ => (None, folder),
    }
}

/// The first part of a folder on `location`: what a catalog's folders begin with.
#[must_use]
pub fn root_of(location: &str) -> String {
    format!("{MARK}{location}")
}

/// Where a statement's whole result goes when its table is in `folder`: a `results` folder beside the data, in the same store.
#[must_use]
pub fn results_of(folder: &str) -> String {
    match location_of(folder) {
        Some(location) => format!("{}/results", root_of(location)),
        None => "results".to_owned(),
    }
}

/// The location a folder is on, if it is on one.
#[must_use]
pub fn location_of(folder: &str) -> Option<&str> {
    split(folder).0
}

fn unavailable() -> FileError {
    FileError::Io(std::io::Error::other(
        "the storage location is not available",
    ))
}

impl Places {
    #[must_use]
    pub fn new(own: Store, own_issuer: Option<Arc<dyn Issuer>>) -> Self {
        let own_place = own.bucket_place().map(|(endpoint, bucket)| Place {
            endpoint: endpoint.to_owned(),
            region: "auto".to_owned(),
            bucket: bucket.to_owned(),
        });
        Self {
            own,
            own_issuer,
            own_place,
            source: OnceLock::new(),
            kept: Mutex::new(HashMap::new()),
        }
    }

    /// Says where locations are known. Done once, at start.
    pub fn attach(&self, source: Arc<dyn LocationSource>) {
        let _ = self.source.set(source);
    }

    /// The installation's own store (where a catalog with no location is).
    #[must_use]
    pub fn own(&self) -> &Store {
        &self.own
    }

    /// Whether the installation's own store can give other machines access to data.
    #[must_use]
    pub fn own_can_vend(&self) -> bool {
        self.own_issuer.is_some()
    }

    /// Forgets a location's built store (it was changed or deleted).
    pub fn forget(&self, location: &str) {
        if let Ok(mut kept) = self.kept.lock() {
            kept.remove(location);
        }
    }

    async fn built(&self, location: &str) -> Result<(Arc<LocationHandle>, String), FileError> {
        let source = self.source.get().ok_or_else(unavailable)?;
        let generation = source
            .generation(location)
            .await
            .map_err(|_| unavailable())?;
        let Some(generation) = generation else {
            self.forget(location);
            return Err(unavailable());
        };
        if let Ok(kept) = self.kept.lock()
            && let Some(k) = kept.get(location)
            && k.generation == generation
            && k.at.elapsed() < LIFE
        {
            return Ok((k.handle.clone(), k.prefix.clone()));
        }
        let opened = source.open(location).await.map_err(|_| unavailable())?;
        let handle = Arc::new(opened.handle);
        if let Ok(mut kept) = self.kept.lock() {
            kept.insert(
                location.to_owned(),
                Kept {
                    at: Instant::now(),
                    generation: handle.generation,
                    handle: handle.clone(),
                    prefix: opened.prefix.clone(),
                },
            );
        }
        Ok((handle, opened.prefix))
    }

    /// Where `folder` is.
    ///
    /// # Errors
    /// The folder is on a location that is gone, or whose credential cannot be used now.
    pub async fn spot(&self, folder: &str) -> Result<Spot, FileError> {
        match split(folder) {
            (None, _) => Ok(Spot {
                store: self.own.clone(),
                issuer: self.own_issuer.clone(),
                rest: folder.to_owned(),
                key: folder.to_owned(),
                location: None,
                place: self.own_place.clone(),
            }),
            (Some(location), rest) => {
                let (handle, prefix) = self.built(location).await?;
                let key = if prefix.is_empty() {
                    rest.to_owned()
                } else {
                    format!("{prefix}/{rest}")
                };
                Ok(Spot {
                    store: handle.store.clone(),
                    issuer: handle.vendor.clone(),
                    rest: rest.to_owned(),
                    key,
                    location: Some(location.to_owned()),
                    place: Some(handle.place()),
                })
            }
        }
    }

    /// The address a client names the folder by.
    ///
    /// # Errors
    /// The folder is not allowed, or its location cannot be reached.
    pub async fn uri(&self, folder: &str) -> Result<String, FileError> {
        let s = self.spot(folder).await?;
        s.store.uri(&s.rest)
    }

    /// # Errors
    /// As [`Store::begin_put`], or the folder's location cannot be reached.
    pub async fn begin_put(&self, folder: &str, rel: &str, max: u64) -> Result<Upload, FileError> {
        let s = self.spot(folder).await?;
        s.store.begin_put(&s.rest, rel, max).await
    }

    /// # Errors
    /// As [`Store::open`], or the folder's location cannot be reached.
    pub async fn open(&self, folder: &str, rel: &str) -> Result<(Reading, u64), FileError> {
        let s = self.spot(folder).await?;
        s.store.open(&s.rest, rel).await
    }

    /// # Errors
    /// As [`Store::delete`], or the folder's location cannot be reached.
    pub async fn delete(&self, folder: &str, rel: &str) -> Result<(), FileError> {
        let s = self.spot(folder).await?;
        s.store.delete(&s.rest, rel).await
    }

    /// # Errors
    /// As [`Store::list`], or the folder's location cannot be reached.
    pub async fn list(&self, folder: &str) -> Result<Vec<Entry>, FileError> {
        let s = self.spot(folder).await?;
        s.store.list(&s.rest).await
    }

    /// # Errors
    /// As [`Store::remove_volume`], or the folder's location cannot be reached.
    pub async fn remove_volume(&self, folder: &str) -> Result<(), FileError> {
        let s = self.spot(folder).await?;
        s.store.remove_volume(&s.rest).await
    }

    /// # Errors
    /// As [`Store::exists`], or the folder's location cannot be reached.
    pub async fn exists(&self, folder: &str) -> Result<bool, FileError> {
        let s = self.spot(folder).await?;
        s.store.exists(&s.rest).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_on_a_location_says_so_in_its_first_segment() {
        let id = "0b9c5e44-7a2f-4a4b-9d0a-0123456789ab";
        let on = format!("{}/cat/tbl", root_of(id));
        assert_eq!(location_of(&on), Some(id));
        assert_eq!(split(&on).1, "cat/tbl");
        assert_eq!(location_of("wk-1/cat/tbl"), None);
        assert_eq!(split("wk-1/cat/tbl"), (None, "wk-1/cat/tbl"));
        // Only the marker and an id make it a location: a bare marker is an ordinary folder name.
        assert_eq!(location_of("loc-"), None);
        assert_eq!(location_of(&root_of(id)), Some(id));
        assert_eq!(results_of(&on), format!("loc-{id}/results"));
        assert_eq!(results_of("wk-1/cat/tbl"), "results");
    }
}
