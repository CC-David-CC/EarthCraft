// SPDX-License-Identifier: MIT
//! The imagery disk cache in Playground's browser build (patches/google-earth.yaml in github.com/daijro/openplayground):
//! it keeps nothing. The desktop keeps fetched tiles in MBTiles files through SQLite on a worker thread, which a page
//! has neither of; here the browser's own HTTP cache keeps the tiles it fetched, and decoded tiles stay in the
//! in-memory cache (tiles/cache.rs) as on the desktop. Same calls as mbtiles.rs.
use std::path::{Path, PathBuf};

pub(super) struct Mbtiles {
    limit: u64,
}

impl Mbtiles {
    #[cfg(test)]
    pub(super) fn new(_root: PathBuf) -> Self {
        Self {
            limit: super::DEFAULT_DISK_BUDGET_BYTES,
        }
    }
    pub(super) fn with_limit(_root: PathBuf, limit: u64) -> Self {
        Self { limit }
    }
    pub(super) fn limit(&self) -> u64 {
        self.limit
    }
    pub(super) async fn read(&self, _path: &Path) -> Result<Option<Vec<u8>>, crate::Error> {
        Ok(None)
    }
    pub(super) async fn write_owned(
        &self,
        _path: &Path,
        _bytes: Vec<u8>,
    ) -> Result<(), crate::Error> {
        Ok(())
    }
    pub(super) async fn invalidate(&self, _path: &Path) -> Result<(), crate::Error> {
        Ok(())
    }
    pub(super) async fn clear(&self) -> Result<(), crate::Error> {
        Ok(())
    }
    pub(super) async fn usage(&self) -> Result<u64, crate::Error> {
        Ok(0)
    }
}
