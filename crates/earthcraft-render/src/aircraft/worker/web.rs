// SPDX-License-Identifier: MIT
//! Aircraft projection in Playground's browser build (patches/google-earth.yaml in github.com/daijro/openplayground):
//! a page has one thread, so a submitted view is projected at once, by the same preparer, and handed over on the app's
//! next take. Same calls as the worker in worker.rs. (The aircraft layer itself is off on the web: its feeds don't
//! answer other sites' pages.)
use super::{AircraftGeometry, AircraftPreparer, ProjectionInput};
use std::cell::RefCell;
use std::sync::Arc;

pub struct AircraftWorker {
    preparer: RefCell<AircraftPreparer>,
    completed: RefCell<Option<Arc<AircraftGeometry>>>,
    repaint: Box<dyn Fn()>,
}

impl AircraftWorker {
    pub fn new(repaint: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        Ok(Self {
            preparer: RefCell::default(),
            completed: RefCell::default(),
            repaint: Box::new(repaint),
        })
    }
    pub fn submit(&self, input: ProjectionInput) {
        let geometry = self.preparer.borrow_mut().prepare(input);
        *self.completed.borrow_mut() = Some(geometry);
        (self.repaint)();
    }
    pub fn take_completed(&self) -> Option<Arc<AircraftGeometry>> {
        self.completed.take()
    }
}
