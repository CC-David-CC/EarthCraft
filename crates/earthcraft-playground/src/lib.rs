// SPDX-License-Identifier: MIT
//! EarthCraft in the browser, for Playground (patches/google-earth.yaml in github.com/daijro/openplayground).
//!
//! wasm32-unknown-unknown has no clock, no files, no threads and no process: `Instant::now` and `SystemTime::now`
//! panic there, every file call fails (`path::absolute` too), and `env::temp_dir` and `process::id` panic. The modules
//! that use them take `std` from here instead, through one `#[cfg(target_arch = "wasm32")] use
//! earthcraft_playground::std;` line at each one's end (a local `std` shadows the standard library's name in that
//! module; everything else in it still comes from the real std):
//!   time     web-time's Instant and SystemTime;
//!   fs       an in-memory file system (settings, caches, KML and screenshots), each change noted for the page to
//!            mirror to Playground's Files;
//!   path     `absolute` is the path from `/`;
//!   thread   `spawn` takes the work but never runs it: the one service that still asks for a thread here (the
//!            settings writer) waits for work in a loop, which a page's one thread can't do;
//!   env      `HOME` is `/` and `temp_dir` is `/tmp`, both in memory (settings and caches go to `/.cache`, which
//!            every page starts without; screenshots and saved places to `/EarthCraft`, a folder of Files too);
//!   process  `id` is 1.
//! And [`moka`]: Moka's cache reads std's clock on every call, so the decoded-tile cache (crates/earthcraft-data's
//! tiles/cache.rs) takes this stand-in on wasm: the same weighted capacity, evicting the least recently used.
#![cfg(target_arch = "wasm32")]

pub mod std {
    pub use ::std::*;

    pub mod time {
        pub use web_time::*;
    }

    pub mod path {
        pub use ::std::path::*;

        /// Paths start at `/` in memory (std's has no current folder on wasm and fails).
        pub fn absolute<P: AsRef<Path>>(path: P) -> ::std::io::Result<PathBuf> {
            Ok(super::fs::key(path.as_ref()))
        }
    }

    pub mod process {
        pub use ::std::process::*;

        /// The page is one process (std's panics on wasm).
        pub fn id() -> u32 {
            1
        }
    }

    pub mod env {
        pub use ::std::env::*;
        use ::std::ffi::{OsStr, OsString};
        use ::std::path::PathBuf;

        /// A folder in the in-memory files (std's panics on wasm).
        pub fn temp_dir() -> PathBuf {
            PathBuf::from("/tmp")
        }

        /// `HOME` is `/`: settings and caches go to the in-memory files. Nothing else is set.
        pub fn var_os<K: AsRef<OsStr>>(key: K) -> Option<OsString> {
            (key.as_ref() == "HOME").then(|| OsString::from("/"))
        }

        pub fn var<K: AsRef<OsStr>>(key: K) -> Result<String, VarError> {
            var_os(key)
                .map(|v| v.to_string_lossy().into_owned())
                .ok_or(VarError::NotPresent)
        }
    }

    pub mod thread {
        use ::std::io;
        pub use ::std::thread::*;

        /// Takes the thread's work and drops it unrun: see the crate's notes.
        pub fn spawn<F, T>(f: F) -> JoinHandle<T>
        where
            F: FnOnce() -> T + Send + 'static,
            T: Send + 'static,
        {
            drop(f);
            JoinHandle(::std::marker::PhantomData)
        }

        /// What [`spawn`] gives back: a thread that never ran.
        pub struct JoinHandle<T>(::std::marker::PhantomData<T>);
        impl<T> JoinHandle<T> {
            pub fn join(self) -> Result<T> {
                Err(Box::new("a page runs no threads"))
            }
            pub fn is_finished(&self) -> bool {
                true
            }
        }

        #[derive(Default)]
        pub struct Builder;
        impl Builder {
            pub fn new() -> Self {
                Builder
            }
            pub fn name(self, _: String) -> Self {
                self
            }
            pub fn stack_size(self, _: usize) -> Self {
                self
            }
            pub fn spawn<F, T>(self, f: F) -> io::Result<JoinHandle<T>>
            where
                F: FnOnce() -> T + Send + 'static,
                T: Send + 'static,
            {
                Ok(spawn(f))
            }
        }
    }

    pub mod fs {
        use ::std::cell::RefCell;
        use ::std::collections::{BTreeMap, BTreeSet};
        pub use ::std::fs::*;
        use ::std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
        use ::std::path::{Component, Path, PathBuf};

        enum Entry {
            Dir,
            File(Vec<u8>, web_time::SystemTime),
        }

        #[derive(Default)]
        struct Tree {
            entries: BTreeMap<PathBuf, Entry>,
            /// Paths written, created or removed since the last [`changes`].
            changed: BTreeSet<PathBuf>,
        }

        thread_local! {
            static TREE: RefCell<Tree> = RefCell::default();
        }

        fn with<T>(f: impl FnOnce(&mut Tree) -> T) -> T {
            TREE.with(|t| f(&mut t.borrow_mut()))
        }

        /// `/a/./b/../c` -> `/a/c`: one spelling per file (relative paths start at `/`).
        pub fn key(p: &Path) -> PathBuf {
            let mut out = PathBuf::from("/");
            for c in p.components() {
                match c {
                    Component::ParentDir => {
                        out.pop();
                    }
                    Component::Normal(n) => out.push(n),
                    _ => {}
                }
            }
            out
        }

        fn missing(p: &Path) -> io::Error {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} does not exist", p.display()),
            )
        }

        impl Tree {
            fn mark(&mut self, p: PathBuf) {
                self.changed.insert(p);
            }
            fn parents(&mut self, p: &Path) {
                for dir in p.ancestors().skip(1) {
                    self.entries.entry(dir.to_path_buf()).or_insert(Entry::Dir);
                }
            }
            fn put(&mut self, p: PathBuf, bytes: Vec<u8>) {
                self.parents(&p);
                self.entries
                    .insert(p, Entry::File(bytes, web_time::SystemTime::now()));
            }
            /// `p` and everything below it.
            fn below(&self, p: &Path) -> Vec<PathBuf> {
                self.entries
                    .range(p.to_path_buf()..)
                    .map(|(k, _)| k)
                    .take_while(|k| k.starts_with(p))
                    .cloned()
                    .collect()
            }
        }

        /// Stores a file without noting a change (files the page brings in from Files or a drop).
        pub fn put(p: &Path, bytes: Vec<u8>) {
            with(|t| t.put(key(p), bytes));
        }

        /// The paths changed since the last call: each now holds what the page's storage should hold, or is gone.
        pub fn changes() -> Vec<PathBuf> {
            with(|t| ::std::mem::take(&mut t.changed).into_iter().collect())
        }

        pub fn exists<P: AsRef<Path>>(p: P) -> io::Result<bool> {
            Ok(metadata(p).is_ok())
        }

        pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(p: P, bytes: C) -> io::Result<()> {
            let p = key(p.as_ref());
            with(|t| {
                t.put(p.clone(), bytes.as_ref().to_vec());
                t.mark(p);
            });
            Ok(())
        }

        pub fn read<P: AsRef<Path>>(p: P) -> io::Result<Vec<u8>> {
            TREE.with(|t| match t.borrow().entries.get(&key(p.as_ref())) {
                Some(Entry::File(bytes, _)) => Ok(bytes.clone()),
                _ => Err(missing(p.as_ref())),
            })
        }

        pub fn read_to_string<P: AsRef<Path>>(p: P) -> io::Result<String> {
            String::from_utf8(read(p)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }

        pub fn create_dir_all<P: AsRef<Path>>(p: P) -> io::Result<()> {
            let p = key(p.as_ref());
            with(|t| {
                if !t.entries.contains_key(&p) {
                    t.parents(&p);
                    t.entries.insert(p.clone(), Entry::Dir);
                    t.mark(p);
                }
            });
            Ok(())
        }

        pub fn create_dir<P: AsRef<Path>>(p: P) -> io::Result<()> {
            if metadata(p.as_ref()).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} exists", p.as_ref().display()),
                ));
            }
            create_dir_all(p)
        }

        pub fn remove_file<P: AsRef<Path>>(p: P) -> io::Result<()> {
            let p = key(p.as_ref());
            with(|t| match t.entries.get(&p) {
                Some(Entry::File(..)) => {
                    t.entries.remove(&p);
                    t.mark(p);
                    Ok(())
                }
                _ => Err(missing(&p)),
            })
        }

        pub fn remove_dir_all<P: AsRef<Path>>(p: P) -> io::Result<()> {
            let p = key(p.as_ref());
            with(|t| {
                let gone = t.below(&p);
                if gone.is_empty() {
                    return Err(missing(&p));
                }
                for g in gone {
                    t.entries.remove(&g);
                    t.mark(g);
                }
                Ok(())
            })
        }

        pub fn rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<()> {
            let (from, to) = (key(from.as_ref()), key(to.as_ref()));
            let bytes = read(&from)?;
            with(|t| {
                t.entries.remove(&from);
                t.mark(from);
                t.put(to.clone(), bytes);
                t.mark(to);
            });
            Ok(())
        }

        pub fn copy<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<u64> {
            let bytes = read(from)?;
            let n = bytes.len() as u64;
            write(to, bytes)?;
            Ok(n)
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct FileType(bool);
        impl FileType {
            pub fn is_file(&self) -> bool {
                self.0
            }
            pub fn is_dir(&self) -> bool {
                !self.0
            }
            pub fn is_symlink(&self) -> bool {
                false
            }
        }

        #[derive(Clone, Debug)]
        pub struct Metadata(Option<(u64, web_time::SystemTime)>);
        impl Metadata {
            pub fn len(&self) -> u64 {
                self.0.map_or(0, |(n, _)| n)
            }
            pub fn is_file(&self) -> bool {
                self.0.is_some()
            }
            pub fn is_dir(&self) -> bool {
                self.0.is_none()
            }
            pub fn is_symlink(&self) -> bool {
                false
            }
            pub fn file_type(&self) -> FileType {
                FileType(self.is_file())
            }
            pub fn modified(&self) -> io::Result<web_time::SystemTime> {
                Ok(self.0.map_or(web_time::SystemTime::UNIX_EPOCH, |(_, t)| t))
            }
        }

        pub fn metadata<P: AsRef<Path>>(p: P) -> io::Result<Metadata> {
            TREE.with(|t| match t.borrow().entries.get(&key(p.as_ref())) {
                Some(Entry::File(bytes, at)) => Ok(Metadata(Some((bytes.len() as u64, *at)))),
                Some(Entry::Dir) => Ok(Metadata(None)),
                None => Err(missing(p.as_ref())),
            })
        }

        /// No links in memory: the same as [`metadata`].
        pub fn symlink_metadata<P: AsRef<Path>>(p: P) -> io::Result<Metadata> {
            metadata(p)
        }

        pub struct DirEntry(PathBuf);
        impl DirEntry {
            pub fn path(&self) -> PathBuf {
                self.0.clone()
            }
            pub fn file_name(&self) -> ::std::ffi::OsString {
                self.0.file_name().unwrap_or_default().to_os_string()
            }
            pub fn metadata(&self) -> io::Result<Metadata> {
                metadata(&self.0)
            }
            pub fn file_type(&self) -> io::Result<FileType> {
                Ok(metadata(&self.0)?.file_type())
            }
        }

        pub type ReadDir = ::std::vec::IntoIter<io::Result<DirEntry>>;

        pub fn read_dir<P: AsRef<Path>>(p: P) -> io::Result<ReadDir> {
            let dir = key(p.as_ref());
            if !metadata(&dir)?.is_dir() {
                return Err(io::Error::other(format!(
                    "{} is not a folder",
                    dir.display()
                )));
            }
            let entries = TREE.with(|t| {
                t.borrow()
                    .below(&dir)
                    .into_iter()
                    .filter(|k| k.parent() == Some(&dir))
                    .map(|k| Ok(DirEntry(k)))
                    .collect::<Vec<_>>()
            });
            Ok(entries.into_iter())
        }

        /// A file's bytes, read whole when it opens; one opened for writing is stored back when flushed or dropped.
        pub struct File {
            data: Cursor<Vec<u8>>,
            /// Where written bytes go, for a file opened to write.
            to: Option<PathBuf>,
            dirty: bool,
        }

        impl File {
            pub fn open<P: AsRef<Path>>(p: P) -> io::Result<File> {
                Ok(File {
                    data: Cursor::new(read(p)?),
                    to: None,
                    dirty: false,
                })
            }
            pub fn create<P: AsRef<Path>>(p: P) -> io::Result<File> {
                write(p.as_ref(), [])?;
                Ok(File {
                    data: Cursor::new(Vec::new()),
                    to: Some(key(p.as_ref())),
                    dirty: false,
                })
            }
            pub fn metadata(&self) -> io::Result<Metadata> {
                Ok(Metadata(Some((
                    self.data.get_ref().len() as u64,
                    web_time::SystemTime::now(),
                ))))
            }
            pub fn sync_all(&self) -> io::Result<()> {
                Ok(())
            }
            fn store(&mut self) {
                if let (true, Some(to)) = (self.dirty, &self.to) {
                    let _ = write(to, self.data.get_ref());
                    self.dirty = false;
                }
            }
        }

        impl Read for File {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                self.data.read(buf)
            }
        }

        impl Seek for File {
            fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
                self.data.seek(pos)
            }
        }

        impl Write for File {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                if self.to.is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "the file is open to read",
                    ));
                }
                self.dirty = true;
                self.data.write(buf)
            }
            fn flush(&mut self) -> io::Result<()> {
                self.store();
                Ok(())
            }
        }

        impl Drop for File {
            fn drop(&mut self) {
                self.store();
            }
        }

        /// `create_new` (screenshots never overwrite) and plain writes; reads go through [`File::open`].
        #[derive(Clone, Debug, Default)]
        pub struct OpenOptions {
            write: bool,
            create_new: bool,
        }
        impl OpenOptions {
            pub fn new() -> Self {
                Self::default()
            }
            pub fn read(&mut self, _: bool) -> &mut Self {
                self
            }
            pub fn write(&mut self, write: bool) -> &mut Self {
                self.write = write;
                self
            }
            pub fn create(&mut self, _: bool) -> &mut Self {
                self
            }
            pub fn truncate(&mut self, _: bool) -> &mut Self {
                self
            }
            pub fn create_new(&mut self, create_new: bool) -> &mut Self {
                self.create_new = create_new;
                self
            }
            pub fn open<P: AsRef<Path>>(&self, p: P) -> io::Result<File> {
                if self.create_new && metadata(p.as_ref()).is_ok() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("{} exists", p.as_ref().display()),
                    ));
                }
                if self.write {
                    File::create(p)
                } else {
                    File::open(p)
                }
            }
        }
    }
}

/// Moka's sync cache, as tiles/cache.rs uses it: a weighted capacity, least recently used out first.
pub mod moka {
    pub mod policy {
        /// Only least-recently-used eviction is kept here, whichever is asked for.
        #[derive(Clone, Copy, Debug, Default)]
        pub struct EvictionPolicy;
        impl EvictionPolicy {
            pub fn lru() -> Self {
                EvictionPolicy
            }
            pub fn tiny_lfu() -> Self {
                EvictionPolicy
            }
        }
    }

    pub mod sync {
        use super::policy::EvictionPolicy;
        use ::std::collections::HashMap;
        use ::std::hash::{BuildHasher, Hash, RandomState};
        use ::std::marker::PhantomData;
        use ::std::sync::{Arc, Mutex};

        type Weigher<K, V> = Box<dyn Fn(&K, &V) -> u32 + Send + Sync>;

        struct Entries<K, V, S> {
            map: HashMap<Arc<K>, (V, u32, u64), S>,
            weight: u64,
            tick: u64,
        }

        pub struct Cache<K, V, S = RandomState> {
            entries: Mutex<Entries<K, V, S>>,
            capacity: u64,
            weigher: Option<Weigher<K, V>>,
        }

        pub struct CacheBuilder<K, V, C> {
            capacity: u64,
            weigher: Option<Weigher<K, V>>,
            cache: PhantomData<C>,
        }

        impl<K, V, C> CacheBuilder<K, V, C>
        where
            K: Hash + Eq + Send + Sync + 'static,
            V: Clone + Send + Sync + 'static,
        {
            pub fn name(self, _: &str) -> Self {
                self
            }
            pub fn max_capacity(mut self, capacity: u64) -> Self {
                self.capacity = capacity;
                self
            }
            pub fn weigher(
                mut self,
                weigher: impl Fn(&K, &V) -> u32 + Send + Sync + 'static,
            ) -> Self {
                self.weigher = Some(Box::new(weigher));
                self
            }
            pub fn eviction_policy(self, _: EvictionPolicy) -> Self {
                self
            }
            pub fn build_with_hasher<S: BuildHasher + Clone + Send + Sync + 'static>(
                self,
                hasher: S,
            ) -> Cache<K, V, S> {
                Cache {
                    entries: Mutex::new(Entries {
                        map: HashMap::with_hasher(hasher),
                        weight: 0,
                        tick: 0,
                    }),
                    capacity: self.capacity,
                    weigher: self.weigher,
                }
            }
            pub fn build(self) -> Cache<K, V> {
                self.build_with_hasher(RandomState::new())
            }
        }

        impl<K, V> Cache<K, V>
        where
            K: Hash + Eq + Send + Sync + 'static,
            V: Clone + Send + Sync + 'static,
        {
            pub fn builder() -> CacheBuilder<K, V, Cache<K, V>> {
                CacheBuilder {
                    capacity: u64::MAX,
                    weigher: None,
                    cache: PhantomData,
                }
            }
        }

        impl<K, V, S> Cache<K, V, S>
        where
            K: Hash + Eq + Send + Sync + 'static,
            V: Clone + Send + Sync + 'static,
            S: BuildHasher + Clone + Send + Sync + 'static,
        {
            pub fn get(&self, key: &K) -> Option<V> {
                let mut e = self.entries.lock().unwrap();
                e.tick += 1;
                let tick = e.tick;
                e.map.get_mut(key).map(|(v, _, used)| {
                    *used = tick;
                    v.clone()
                })
            }
            pub fn contains_key(&self, key: &K) -> bool {
                self.entries.lock().unwrap().map.contains_key(key)
            }
            pub fn insert(&self, key: K, value: V) {
                let weight = self.weigher.as_ref().map_or(1, |w| w(&key, &value));
                let mut e = self.entries.lock().unwrap();
                e.tick += 1;
                let tick = e.tick;
                if let Some((_, old, _)) = e.map.insert(Arc::new(key), (value, weight, tick)) {
                    e.weight -= old as u64;
                }
                e.weight += weight as u64;
                // ponytail: a full scan per eviction (a few thousand tiles at most); a heap if it ever shows in profiles.
                while e.weight > self.capacity {
                    let Some(oldest) = e
                        .map
                        .iter()
                        .min_by_key(|(_, (_, _, used))| *used)
                        .map(|(k, _)| k.clone())
                    else {
                        break;
                    };
                    if let Some((_, w, _)) = e.map.remove(&oldest) {
                        e.weight -= w as u64;
                    }
                }
            }
            pub fn invalidate(&self, key: &K) {
                let mut e = self.entries.lock().unwrap();
                if let Some((_, w, _)) = e.map.remove(key) {
                    e.weight -= w as u64;
                }
            }
            pub fn invalidate_all(&self) {
                let mut e = self.entries.lock().unwrap();
                e.map.clear();
                e.weight = 0;
            }
            pub fn weighted_size(&self) -> u64 {
                self.entries.lock().unwrap().weight
            }
            pub fn entry_count(&self) -> u64 {
                self.entries.lock().unwrap().map.len() as u64
            }
            pub fn run_pending_tasks(&self) {}
            /// A snapshot, as Moka's iterator is.
            pub fn iter(&self) -> ::std::vec::IntoIter<(Arc<K>, V)> {
                let e = self.entries.lock().unwrap();
                e.map
                    .iter()
                    .map(|(k, (v, _, _))| (k.clone(), v.clone()))
                    .collect::<Vec<_>>()
                    .into_iter()
            }
        }
    }
}
