// SPDX-License-Identifier: MIT
//! Border and label preparation in Playground's browser build (patches/google-earth.yaml in
//! github.com/daijro/openplayground): a page has one thread, so a submitted view is prepared at once, by the same
//! preparer, and handed over on the app's next take. Same calls as the worker in worker.rs.
use super::{OverlayGeometry, OverlayInput, OverlayPreparer, OverlayView};
use earthcraft_data::boundaries::BoundaryData;
use std::cell::RefCell;
use std::sync::Arc;

pub struct OverlayWorker {
    preparer: RefCell<OverlayPreparer>,
    completed: RefCell<Option<Arc<OverlayGeometry>>>,
    repaint: Box<dyn Fn()>,
}

impl OverlayWorker {
    pub fn new(repaint: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        Ok(Self {
            preparer: RefCell::default(),
            completed: RefCell::default(),
            repaint: Box::new(repaint),
        })
    }
    pub fn submit(&self, view: OverlayView, data: Arc<BoundaryData>) {
        let geometry = self
            .preparer
            .borrow_mut()
            .prepare(OverlayInput { view, data });
        *self.completed.borrow_mut() = Some(geometry);
        (self.repaint)();
    }
    pub fn take_completed(&self) -> Option<Arc<OverlayGeometry>> {
        self.completed.take()
    }
}
