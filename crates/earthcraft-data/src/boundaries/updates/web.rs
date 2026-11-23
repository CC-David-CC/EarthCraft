// SPDX-License-Identifier: MIT
//! Boundary patches in Playground's browser build (patches/google-earth.yaml in github.com/daijro/openplayground): the
//! same ordered merge as the desktop's preparation thread, done when the app asks for the latest snapshot, since a
//! page has one thread. The loaders wake the app after each layer they send (boundaries/load.rs), as on the desktop.
use super::{BoundaryData, BoundarySnapshot, Patch, apply};
use std::cell::RefCell;
use std::sync::{Arc, mpsc};

pub struct BoundaryUpdates {
    patches: mpsc::Receiver<Patch>,
    data: RefCell<Arc<BoundaryData>>,
}

impl BoundaryUpdates {
    pub fn new(
        patches: mpsc::Receiver<Patch>,
        _repaint: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        Ok(Self {
            patches,
            data: RefCell::default(),
        })
    }

    /// Merges the patches that arrived, in order, into one snapshot.
    pub fn take_latest(&self) -> Option<BoundarySnapshot> {
        let mut data = self.data.borrow_mut();
        let mut changed = false;
        for patch in self.patches.try_iter() {
            apply(Arc::make_mut(&mut data), patch);
            changed = true;
        }
        changed.then(|| BoundarySnapshot {
            data: data.clone(),
            names: Arc::new(data.labels.clone()),
        })
    }
}
