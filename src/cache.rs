use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::Tileset;

/// A reference type that is used to refer to a resource. For the owned variant, see [`ResourcePathBuf`].
pub type ResourcePath = Path;
/// An owned type that is used to refer to a resource. For the non-owned variant, see [`ResourcePath`].
pub type ResourcePathBuf = PathBuf;

/// A trait identifying a data type that holds resources (such as tilesets) and maps them to a
/// [`ResourcePath`] to prevent loading them more than once.
pub trait ResourceCache {
    /// Obtains a tileset from the cache, if it exists.
    /// 
    /// # Example
    /// ```
    /// use std::fs::File;
    /// use tiled::{FilesystemResourceCache, ResourceCache, Tileset};
    /// # use tiled::TiledError;
    /// # fn main() -> Result<(), TiledError> {
    /// let mut cache = FilesystemResourceCache::new();
    /// let path = "assets/tilesheet.tsx";
    ///
    /// assert!(cache.get_tileset(path).is_none());
    /// cache.get_or_try_insert_tileset_with(path.to_owned().into(), || Tileset::parse_reader(File::open(path).unwrap(), path))?;
    /// assert!(cache.get_tileset(path).is_some());
    /// # Ok(())
    /// # }
    /// ```
    fn get_tileset(&self, path: impl AsRef<ResourcePath>) -> Option<Arc<Tileset>>;
    
    /// Returns the tileset mapped to `path` if it exists, otherwise calls `f` and, depending on its
    /// result, it will:
    /// - Insert the object into the cache, if the result was [`Ok`].
    /// - Return the error and leave the cache intact, if the result was [`Err`].
    fn get_or_try_insert_tileset_with<F, E>(
        &mut self,
        path: ResourcePathBuf,
        f: F,
    ) -> Result<Arc<Tileset>, E>
    where
        F: FnOnce() -> Result<Tileset, E>;
}

/// A cache that identifies resources by their path in the user's filesystem.
#[derive(Debug)]
pub struct FilesystemResourceCache {
    tilesets: HashMap<ResourcePathBuf, Arc<Tileset>>,
}

impl FilesystemResourceCache {
    /// Creates an empty [`FilesystemResourceCache`].
    pub fn new() -> Self {
        Self {
            tilesets: HashMap::new(),
        }
    }
}

impl ResourceCache for FilesystemResourceCache {
    fn get_tileset(&self, path: impl AsRef<ResourcePath>) -> Option<Arc<Tileset>> {
        self.tilesets.get(path.as_ref()).map(Clone::clone)
    }

    fn get_or_try_insert_tileset_with<F, E>(
        &mut self,
        path: ResourcePathBuf,
        f: F,
    ) -> Result<Arc<Tileset>, E>
    where
        F: FnOnce() -> Result<Tileset, E>,
    {
        Ok(match self.tilesets.entry(path) {
            std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => v.insert(Arc::new(f()?)),
        }
        .clone())
    }
}