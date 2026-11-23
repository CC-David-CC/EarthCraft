// SPDX-License-Identifier: MIT
//! EarthCraft in the browser, for Playground (patches/google-earth.yaml in github.com/daijro/openplayground): the part of
//! Tokio crates/earthcraft-data uses, as a page can have it. The crate takes it as `tokio` on wasm (an `extern crate …
//! as tokio` at its root; the real Tokio is a native-only dependency there).
//!
//! Tokio's runtime needs threads and a clock that wasm32-unknown-unknown lacks. Here:
//!   runtime, spawn   tasks run on the page's event loop (wasm-bindgen-futures), so the network waits in the browser
//!                    and the page stays responsive; the runtime's thread counts mean nothing;
//!   spawn_blocking   runs its work at once, on the one thread (the decoders and JSON parsing it is given);
//!   time             timers are the browser's `setTimeout`, and Instant is web-time's (the crate's modules take
//!                    `std::time` from earthcraft-playground too, so the two agree);
//!   fs               the in-memory files of earthcraft-playground's `std::fs`;
//!   sync, io, select!, join!   Tokio's own, which need no runtime.
//! Every future here is Send, as Tokio's are, so the crate's own `Send` bounds hold.
#![cfg(target_arch = "wasm32")]

pub use task::spawn;
pub use tokio::{io, join, pin, select, sync, try_join};

pub mod task {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};
    use tokio::sync::oneshot;

    /// A task stopped by [`JoinHandle::abort`] (a panic stops the whole page instead).
    #[derive(Debug)]
    pub struct JoinError;
    impl std::fmt::Display for JoinError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("task was cancelled")
        }
    }
    impl std::error::Error for JoinError {}
    impl JoinError {
        pub fn is_cancelled(&self) -> bool {
            true
        }
        pub fn is_panic(&self) -> bool {
            false
        }
    }

    #[derive(Default)]
    struct Abort {
        aborted: AtomicBool,
        waker: Mutex<Option<Waker>>,
    }

    /// The task's output, or [`JoinError`] once it is aborted.
    pub struct JoinHandle<T> {
        output: oneshot::Receiver<T>,
        abort: Arc<Abort>,
    }
    impl<T> JoinHandle<T> {
        pub fn abort(&self) {
            self.abort.aborted.store(true, Ordering::Relaxed);
            if let Some(waker) = self.abort.waker.lock().unwrap().take() {
                waker.wake();
            }
        }
        pub fn is_finished(&self) -> bool {
            self.output.is_terminated() || !self.output.is_empty()
        }
    }
    impl<T> Future for JoinHandle<T> {
        type Output = Result<T, JoinError>;
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            Pin::new(&mut self.output)
                .poll(cx)
                .map(|r| r.map_err(|_| JoinError))
        }
    }

    /// Runs `future` on the page's event loop.
    pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        let (send, output) = oneshot::channel();
        let abort = Arc::new(Abort::default());
        let watch = abort.clone();
        let mut future = Box::pin(future);
        wasm_bindgen_futures::spawn_local(async move {
            let finished = std::future::poll_fn(|cx| {
                if watch.aborted.load(Ordering::Relaxed) {
                    return Poll::Ready(None);
                }
                *watch.waker.lock().unwrap() = Some(cx.waker().clone());
                future.as_mut().poll(cx).map(Some)
            })
            .await;
            if let Some(value) = finished {
                let _ = send.send(value);
            }
        });
        JoinHandle { output, abort }
    }

    /// Runs `work` now, on the page's one thread.
    pub fn spawn_blocking<F, T>(work: F) -> JoinHandle<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let (send, output) = oneshot::channel();
        let _ = send.send(work());
        JoinHandle {
            output,
            abort: Arc::default(),
        }
    }

    /// Tasks whose outputs come back in the order they finish.
    pub struct JoinSet<T> {
        tasks: Vec<JoinHandle<T>>,
    }
    impl<T> Default for JoinSet<T> {
        fn default() -> Self {
            Self { tasks: Vec::new() }
        }
    }
    impl<T: 'static> JoinSet<T> {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn spawn<F: Future<Output = T> + 'static>(&mut self, future: F) {
            self.tasks.push(spawn(future));
        }
        pub fn len(&self) -> usize {
            self.tasks.len()
        }
        pub fn is_empty(&self) -> bool {
            self.tasks.is_empty()
        }
        pub fn abort_all(&mut self) {
            self.tasks.iter().for_each(JoinHandle::abort);
        }
        pub async fn join_next(&mut self) -> Option<Result<T, JoinError>> {
            if self.tasks.is_empty() {
                return None;
            }
            std::future::poll_fn(|cx| {
                for i in 0..self.tasks.len() {
                    if let Poll::Ready(output) = Pin::new(&mut self.tasks[i]).poll(cx) {
                        self.tasks.swap_remove(i);
                        return Poll::Ready(Some(output));
                    }
                }
                Poll::Pending
            })
            .await
        }
    }
    impl<T> Drop for JoinSet<T> {
        fn drop(&mut self) {
            self.tasks.iter().for_each(JoinHandle::abort);
        }
    }
}

pub mod runtime {
    use crate::task::{JoinHandle, spawn};
    use std::future::Future;

    /// The page's event loop: there is one, whatever the builder asks for.
    #[derive(Clone, Debug, Default)]
    pub struct Handle;
    impl Handle {
        pub fn current() -> Self {
            Handle
        }
        pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
        {
            spawn(future)
        }
    }

    #[derive(Debug, Default)]
    pub struct Runtime {
        handle: Handle,
    }
    impl Runtime {
        pub fn handle(&self) -> &Handle {
            &self.handle
        }
        pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
        {
            spawn(future)
        }
    }

    #[derive(Debug, Default)]
    pub struct Builder;
    impl Builder {
        pub fn new_multi_thread() -> Self {
            Builder
        }
        pub fn new_current_thread() -> Self {
            Builder
        }
        pub fn worker_threads(&mut self, _: usize) -> &mut Self {
            self
        }
        pub fn max_blocking_threads(&mut self, _: usize) -> &mut Self {
            self
        }
        pub fn thread_name(&mut self, _: impl Into<String>) -> &mut Self {
            self
        }
        pub fn enable_all(&mut self) -> &mut Self {
            self
        }
        pub fn enable_time(&mut self) -> &mut Self {
            self
        }
        pub fn build(&mut self) -> std::io::Result<Runtime> {
            Ok(Runtime::default())
        }
    }
}

pub mod time {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};
    pub use std::time::Duration;
    use wasm_bindgen::{JsCast, JsValue, closure::Closure, prelude::wasm_bindgen};
    pub use web_time::Instant;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_name = setTimeout)]
        fn set_timeout(handler: &js_sys::Function, ms: i32) -> JsValue;
    }

    #[derive(Default)]
    struct Timer {
        fired: AtomicBool,
        waker: Mutex<Option<Waker>>,
    }

    /// Done once the browser's timer fires.
    pub struct Sleep(Arc<Timer>);
    impl Future for Sleep {
        type Output = ();
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            *self.0.waker.lock().unwrap() = Some(cx.waker().clone());
            if self.0.fired.load(Ordering::Relaxed) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }
    }

    pub fn sleep(duration: Duration) -> Sleep {
        let timer = Arc::new(Timer::default());
        let fire = timer.clone();
        let handler = Closure::once_into_js(move || {
            fire.fired.store(true, Ordering::Relaxed);
            if let Some(waker) = fire.waker.lock().unwrap().take() {
                waker.wake();
            }
        });
        set_timeout(
            handler.unchecked_ref(),
            duration.as_millis().min(i32::MAX as u128) as i32,
        );
        Sleep(timer)
    }

    pub fn sleep_until(deadline: Instant) -> Sleep {
        sleep(deadline.saturating_duration_since(Instant::now()))
    }

    pub mod error {
        /// The time ran out before the future finished.
        #[derive(Debug, PartialEq, Eq)]
        pub struct Elapsed;
        impl std::fmt::Display for Elapsed {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("deadline has elapsed")
            }
        }
        impl std::error::Error for Elapsed {}
    }

    pub async fn timeout<F: Future>(
        duration: Duration,
        future: F,
    ) -> Result<F::Output, error::Elapsed> {
        let mut future = std::pin::pin!(future);
        let mut timer = sleep(duration);
        std::future::poll_fn(|cx| {
            if let Poll::Ready(value) = future.as_mut().poll(cx) {
                return Poll::Ready(Ok(value));
            }
            Pin::new(&mut timer).poll(cx).map(|()| Err(error::Elapsed))
        })
        .await
    }
}

/// The in-memory files, as Tokio's async calls.
pub mod fs {
    use earthcraft_playground::std::fs;
    pub use fs::Metadata;
    use std::io::{self, Write as _};
    use std::path::Path;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    pub async fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        fs::read(path)
    }
    pub async fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
        fs::read_to_string(path)
    }
    pub async fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        fs::write(path, bytes)
    }
    pub async fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        fs::create_dir_all(path)
    }
    pub async fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        fs::rename(from, to)
    }
    pub async fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        fs::remove_file(path)
    }
    pub async fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        fs::remove_dir_all(path)
    }
    pub async fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
        fs::metadata(path)
    }

    /// A file being written, stored when flushed or dropped.
    pub struct File(fs::File);
    impl File {
        pub async fn create(path: impl AsRef<Path>) -> io::Result<File> {
            fs::File::create(path).map(File)
        }
    }
    impl tokio::io::AsyncWrite for File {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(self.0.write(buf))
        }
        fn poll_flush(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(self.0.flush())
        }
        fn poll_shutdown(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(self.0.flush())
        }
    }
}
