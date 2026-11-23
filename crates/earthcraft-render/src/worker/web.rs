// SPDX-License-Identifier: MIT
//! The frame coordinator in Playground's browser build (patches/google-earth.yaml in github.com/daijro/openplayground):
//! a page has one thread, so a submitted view is rasterized at once, on it (with Rayon running on that thread too),
//! and published for the app to take on its next frame, as the desktop's coordinator thread publishes it. Same calls
//! as the coordinator in worker.rs; the frame is still one behind the request, and the app still draws reduced
//! rasters while the globe moves (view.rs).
use super::{RenderRequest, RenderedFrame};
use std::cell::RefCell;

type Render = Box<dyn FnMut(&RenderRequest, Vec<u8>) -> crate::RenderedView>;

pub struct RenderWorker<T = ()> {
    render: RefCell<Render>,
    prepare: RefCell<Box<dyn FnMut(&RenderRequest, &[u8]) -> T>>,
    admit: Box<dyn Fn(&crate::RenderedView)>,
    repaint: Box<dyn Fn()>,
    completed: RefCell<Option<RenderedFrame<T>>>,
    spare: RefCell<Option<Vec<u8>>>,
}

impl RenderWorker {
    pub fn new(
        render: impl FnMut(&RenderRequest, Vec<u8>) -> crate::RenderedView + Send + 'static,
        repaint: impl Fn() + Send + 'static,
        admit: impl Fn(&crate::RenderedView) + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::with_prepare(render, repaint, admit, |_, _| ())
    }
}

impl<T: Send + 'static> RenderWorker<T> {
    pub fn with_prepare(
        render: impl FnMut(&RenderRequest, Vec<u8>) -> crate::RenderedView + Send + 'static,
        repaint: impl Fn() + Send + 'static,
        admit: impl Fn(&crate::RenderedView) + Send + 'static,
        prepare: impl FnMut(&RenderRequest, &[u8]) -> T + Send + 'static,
    ) -> std::io::Result<Self> {
        Ok(Self {
            render: RefCell::new(Box::new(render)),
            prepare: RefCell::new(Box::new(prepare)),
            admit: Box::new(admit),
            repaint: Box::new(repaint),
            completed: RefCell::default(),
            spare: RefCell::default(),
        })
    }

    pub fn recycle(&self, buffer: Vec<u8>) {
        *self.spare.borrow_mut() = Some(buffer);
    }

    pub fn submit(&self, request: RenderRequest) {
        let buffer = self.spare.take().unwrap_or_default();
        let frame = (self.render.borrow_mut())(&request, buffer);
        let prepared = (self.prepare.borrow_mut())(&request, &frame.pixels);
        (self.admit)(&frame);
        if let Some(previous) = self.completed.take() {
            *self.spare.borrow_mut() = Some(previous.pixels);
        }
        let required = frame.required_tiles();
        *self.completed.borrow_mut() = Some(RenderedFrame {
            prepared,
            request,
            pixels: frame.pixels,
            waiting: frame.waiting,
            required,
        });
        (self.repaint)();
    }

    pub fn take_completed(&self, presentation: u64) -> Option<RenderedFrame<T>> {
        self.completed
            .take()
            .filter(|frame| frame.request.presentation == presentation)
    }

    pub fn is_idle(&self) -> bool {
        true
    }
}
