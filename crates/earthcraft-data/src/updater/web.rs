// SPDX-License-Identifier: MIT
//! Updates in Playground's browser build (patches/google-earth.yaml in github.com/daijro/openplayground): there are
//! none to install. Playground serves the newest EarthCraft it has built, so the startup check finds the page up to
//! date, and the desktop's download-and-replace (self_update, which has no browser side) is never offered. Same calls
//! as release.rs.
use super::Snapshot;
use std::{path::Path, sync::Arc};
use tokio::sync::watch;

#[derive(Clone)]
pub(super) struct SelectedRelease {
    pub(super) version: String,
}

pub(super) fn is_development_path(_: &Path) -> bool {
    false
}

pub(super) fn manual_install_reason(_: &Path) -> Option<String> {
    None
}

pub(super) async fn check() -> Result<Option<SelectedRelease>, crate::Error> {
    Ok(None)
}

pub(super) async fn install(
    _selected: &SelectedRelease,
    _sender: watch::Sender<Snapshot>,
    _repaint: Arc<dyn Fn() + Send + Sync>,
) -> Result<bool, crate::Error> {
    Ok(false)
}
