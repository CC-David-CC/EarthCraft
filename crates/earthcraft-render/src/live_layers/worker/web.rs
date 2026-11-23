// SPDX-License-Identifier: MIT
//! Live layer preparation (roads, ships, satellites, traffic) in Playground's browser build (patches/google-earth.yaml
//! in github.com/daijro/openplayground): a page has one thread, so a submitted view is prepared at once, by the same
//! preparer, and handed over on the app's next take. Same calls as the worker in worker.rs.
use super::{LiveGeometry, LiveInput, LivePreparer};
use std::cell::RefCell;
use std::sync::Arc;

pub struct LiveWorker {
    preparer: RefCell<LivePreparer>,
    completed: RefCell<Option<Arc<LiveGeometry>>>,
    repaint: Box<dyn Fn()>,
}

impl LiveWorker {
    pub fn new(repaint: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        Ok(Self {
            preparer: RefCell::default(),
            completed: RefCell::default(),
            repaint: Box::new(repaint),
        })
    }
    pub fn submit(&self, input: LiveInput) {
        let geometry = self.preparer.borrow_mut().prepare(input);
        *self.completed.borrow_mut() = Some(geometry);
        (self.repaint)();
    }
    pub fn take_completed(&self) -> Option<Arc<LiveGeometry>> {
        self.completed.take()
    }
}
