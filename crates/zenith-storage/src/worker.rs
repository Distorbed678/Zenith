//! Storage IO worker (3-level strict priority queue, write coalescing, read-after-write deep copy,
//! synchronize/flush, shutdown drain) and world-blending old-chunk scanner.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use zenith_nbt::{Compound, NbtString, TagType};

use crate::cache::RegionCache;
use crate::error::{Result, StorageError};
use crate::layout::{ChunkPos, RegionPos};
use crate::nbt_io::{EntryResult, StreamTagVisitor, ValueResult, scan_compound_in_memory};
use crate::region::RegionFileOptions;

/// LRU cache limit for the world-blending old-chunk region bitset cache (`1,024`).
pub const BLENDER_SCAN_CACHE_CAPACITY: usize = 1_024;

/// `DataVersion` threshold below which a chunk is classified as an "old chunk" for blending (`4,997`).
pub const BLENDER_OLD_CHUNK_VERSION_CUTOFF: i32 = 4_997;

/// Strict priority tiers of the storage IO worker (lower number = higher priority).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(usize)]
pub enum WorkerPriority {
    /// Priority `0` (highest): foreground store enqueue, reads, streaming scans, sync barriers, cache flushes.
    Foreground = 0,
    /// Priority `1`: background write drain (one pending chunk per step).
    Background = 1,
    /// Priority `2` (lowest): shutdown final drain barrier.
    Shutdown = 2,
}

/// Shared error type used by clonable [`IoHandle`] completions (required for coalesced writes).
pub type SharedStorageError = Arc<StorageError>;

struct HandleState<T> {
    result: Mutex<Option<std::result::Result<T, SharedStorageError>>>,
    cvar: Condvar,
}

/// Completion handle returned by [`StorageWorker`] operations.
///
/// Coalesced writes for the same [`ChunkPos`] return clones of the **exact same** `IoHandle`
/// (verifiable via [`IoHandle::same_handle`]).
pub struct IoHandle<T> {
    inner: Arc<HandleState<T>>,
}

impl<T> Clone for IoHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> fmt::Debug for IoHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IoHandle")
            .field("completed", &self.is_complete())
            .finish()
    }
}

impl<T> IoHandle<T> {
    fn new() -> Self {
        Self {
            inner: Arc::new(HandleState {
                result: Mutex::new(None),
                cvar: Condvar::new(),
            }),
        }
    }

    fn complete(&self, value: std::result::Result<T, SharedStorageError>) {
        let mut guard = self.inner.result.lock().expect("IoHandle mutex poisoned");
        if guard.is_none() {
            *guard = Some(value);
            self.inner.cvar.notify_all();
        }
    }

    /// Returns `true` if the operation has completed.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.inner
            .result
            .lock()
            .expect("IoHandle mutex poisoned")
            .is_some()
    }

    /// Returns `true` if `self` and `other` point to the exact same underlying completion handle.
    #[must_use]
    pub fn same_handle(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Blocks until the operation completes and returns a clone of its result.
    pub fn wait(&self) -> std::result::Result<T, SharedStorageError>
    where
        T: Clone,
    {
        let mut guard = self.inner.result.lock().expect("IoHandle mutex poisoned");
        while guard.is_none() {
            guard = self
                .inner
                .cvar
                .wait(guard)
                .expect("IoHandle condvar poisoned");
        }
        guard.as_ref().expect("checked Some").clone()
    }

    /// Waits up to `timeout` for completion, returning `None` if still incomplete.
    pub fn wait_timeout(
        &self,
        timeout: Duration,
    ) -> Option<std::result::Result<T, SharedStorageError>>
    where
        T: Clone,
    {
        let guard = self.inner.result.lock().expect("IoHandle mutex poisoned");
        let (guard, _) = self
            .inner
            .cvar
            .wait_timeout_while(guard, timeout, |state| state.is_none())
            .expect("IoHandle condvar poisoned");
        guard.as_ref().cloned()
    }

    /// Non-blocking poll of the completion result.
    #[must_use]
    pub fn try_get(&self) -> Option<std::result::Result<T, SharedStorageError>>
    where
        T: Clone,
    {
        self.inner
            .result
            .lock()
            .expect("IoHandle mutex poisoned")
            .as_ref()
            .cloned()
    }
}

struct PendingWriteEntry {
    tag: Option<Compound>,
    handle: IoHandle<()>,
    waiters: Vec<IoHandle<()>>,
}

/// Insertion-ordered map from [`ChunkPos`] to `(Option<Compound>, IoHandle<()>)`.
#[derive(Default)]
struct PendingWriteMap {
    entries: HashMap<ChunkPos, PendingWriteEntry>,
    order: VecDeque<ChunkPos>,
}

impl PendingWriteMap {
    fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    fn len(&self) -> usize {
        self.order.len()
    }

    fn get(&self, pos: ChunkPos) -> Option<&Option<Compound>> {
        self.entries.get(&pos).map(|e| &e.tag)
    }

    /// Inserts or overwrites `pos` in place without changing its position in the insertion order,
    /// returning the existing completion handle on overwrite or a fresh one on insert.
    fn upsert(
        &mut self,
        pos: ChunkPos,
        tag: Option<Compound>,
        extra_waiter: Option<IoHandle<()>>,
    ) -> IoHandle<()> {
        if let Some(existing) = self.entries.get_mut(&pos) {
            existing.tag = tag;
            if let Some(w) = extra_waiter {
                existing.waiters.push(w);
            }
            existing.handle.clone()
        } else {
            let handle = IoHandle::new();
            let mut waiters = Vec::new();
            if let Some(w) = extra_waiter {
                waiters.push(w);
            }
            self.entries.insert(
                pos,
                PendingWriteEntry {
                    tag,
                    handle: handle.clone(),
                    waiters,
                },
            );
            self.order.push_back(pos);
            handle
        }
    }

    fn pop_oldest(&mut self) -> Option<(ChunkPos, PendingWriteEntry)> {
        let pos = self.order.pop_front()?;
        let entry = self
            .entries
            .remove(&pos)
            .expect("PendingWriteMap order and entries out of sync");
        Some((pos, entry))
    }

    fn snapshot_handles(&self) -> Vec<IoHandle<()>> {
        self.order
            .iter()
            .filter_map(|pos| self.entries.get(pos).map(|e| e.handle.clone()))
            .collect()
    }
}

struct SyncWaiter {
    handles: Vec<IoHandle<()>>,
    flush: bool,
    result_handle: IoHandle<()>,
}

type WorkerTask = Box<dyn FnOnce(&mut WorkerCore) + Send>;

struct SharedQueue {
    queues: [VecDeque<WorkerTask>; 3],
    stopped: bool,
}

impl SharedQueue {
    fn new() -> Self {
        Self {
            queues: [VecDeque::new(), VecDeque::new(), VecDeque::new()],
            stopped: false,
        }
    }

    fn push(&mut self, priority: WorkerPriority, task: WorkerTask) {
        self.queues[priority as usize].push_back(task);
    }

    fn pop(&mut self) -> Option<(WorkerPriority, WorkerTask)> {
        if let Some(t) = self.queues[WorkerPriority::Foreground as usize].pop_front() {
            return Some((WorkerPriority::Foreground, t));
        }
        if let Some(t) = self.queues[WorkerPriority::Background as usize].pop_front() {
            return Some((WorkerPriority::Background, t));
        }
        if let Some(t) = self.queues[WorkerPriority::Shutdown as usize].pop_front() {
            return Some((WorkerPriority::Shutdown, t));
        }
        None
    }
}

struct WorkerCore {
    cache: RegionCache,
    pending_writes: PendingWriteMap,
    sync_waiters: Vec<SyncWaiter>,
    background_drain_scheduled: bool,
    queue: Arc<(Mutex<SharedQueue>, Condvar)>,
    closed: bool,
}

impl WorkerCore {
    fn new(
        dir: PathBuf,
        options: RegionFileOptions,
        queue: Arc<(Mutex<SharedQueue>, Condvar)>,
    ) -> Self {
        Self {
            cache: RegionCache::new(dir, options),
            pending_writes: PendingWriteMap::default(),
            sync_waiters: Vec::new(),
            background_drain_scheduled: false,
            queue,
            closed: false,
        }
    }

    fn enqueue(&self, priority: WorkerPriority, task: WorkerTask) {
        let (lock, cvar) = &*self.queue;
        let mut q = lock.lock().expect("worker queue mutex poisoned");
        q.push(priority, task);
        cvar.notify_one();
    }

    fn schedule_background_drain(&mut self) {
        if self.background_drain_scheduled || self.pending_writes.is_empty() {
            return;
        }
        self.background_drain_scheduled = true;
        self.enqueue(
            WorkerPriority::Background,
            Box::new(|core| {
                core.run_one_background_drain();
            }),
        );
    }

    fn run_one_background_drain(&mut self) {
        self.background_drain_scheduled = false;
        if let Some((pos, entry)) = self.pending_writes.pop_oldest() {
            let res = self.cache.write(pos, entry.tag.as_ref()).map_err(Arc::new);
            entry.handle.complete(res.clone());
            for waiter in entry.waiters {
                waiter.complete(res.clone());
            }
            self.check_sync_waiters();
            if !self.pending_writes.is_empty() {
                self.schedule_background_drain();
            }
        }
    }

    fn check_sync_waiters(&mut self) {
        let mut idx = 0;
        while idx < self.sync_waiters.len() {
            let all_done = self.sync_waiters[idx]
                .handles
                .iter()
                .all(IoHandle::is_complete);
            if all_done {
                let waiter = self.sync_waiters.remove(idx);
                let flush = waiter.flush;
                let handle = waiter.result_handle;
                self.enqueue(
                    WorkerPriority::Foreground,
                    Box::new(move |core| {
                        let res = if flush {
                            core.cache.flush().map_err(Arc::new)
                        } else {
                            Ok(())
                        };
                        handle.complete(res);
                    }),
                );
            } else {
                idx += 1;
            }
        }
    }

    fn close_cache_swallowing_error(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        // Swap out the cache with an empty one to invoke `RegionCache::close`.
        let empty = RegionCache::with_capacity(self.cache.dir(), RegionFileOptions::default(), 1);
        let old = std::mem::replace(&mut self.cache, empty);
        if let Err(err) = old.close() {
            log::error!("Failed to close region cache on worker shutdown: {err}");
        }
    }
}

enum ExecutionMode {
    /// Synchronous/manually steppable execution on the caller's thread.
    Sync(Mutex<WorkerCore>),
    /// Dedicated background thread draining the 3-level priority queue.
    Threaded {
        join_handle: Mutex<Option<JoinHandle<()>>>,
    },
}

/// Serialized storage IO worker implementing the spec's 3-level strict priority queue,
/// write coalescing, foreground read-after-write deep copies, synchronize/flush barriers,
/// and idempotent shutdown draining.
///
/// Supports two execution modes:
/// - [`StorageWorker::new_sync`]: single-threaded synchronous & manually steppable mode.
/// - [`StorageWorker::spawn`]: optional dedicated background thread mode.
pub struct StorageWorker {
    dir: PathBuf,
    queue: Arc<(Mutex<SharedQueue>, Condvar)>,
    shutting_down: Arc<AtomicBool>,
    mode: ExecutionMode,
}

impl fmt::Debug for StorageWorker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageWorker")
            .field("dir", &self.dir)
            .field("shutting_down", &self.shutting_down.load(Ordering::Acquire))
            .field(
                "threaded",
                &matches!(self.mode, ExecutionMode::Threaded { .. }),
            )
            .finish()
    }
}

impl StorageWorker {
    /// Creates a synchronous (manually steppable / on-demand driven) storage worker.
    #[must_use]
    pub fn new_sync(dir: impl Into<PathBuf>, options: RegionFileOptions) -> Self {
        let dir = dir.into();
        let queue = Arc::new((Mutex::new(SharedQueue::new()), Condvar::new()));
        let shutting_down = Arc::new(AtomicBool::new(false));
        let core = WorkerCore::new(dir.clone(), options, Arc::clone(&queue));
        Self {
            dir,
            queue,
            shutting_down,
            mode: ExecutionMode::Sync(Mutex::new(core)),
        }
    }

    /// Creates a storage worker backed by a dedicated background thread.
    pub fn spawn(dir: impl Into<PathBuf>, options: RegionFileOptions) -> Result<Self> {
        let dir = dir.into();
        let queue = Arc::new((Mutex::new(SharedQueue::new()), Condvar::new()));
        let shutting_down = Arc::new(AtomicBool::new(false));

        let thread_queue = Arc::clone(&queue);
        let thread_shutdown = Arc::clone(&shutting_down);
        let thread_dir = dir.clone();

        let join_handle = thread::Builder::new()
            .name("zenith-storage-io".to_owned())
            .spawn(move || {
                let mut core = WorkerCore::new(thread_dir, options, Arc::clone(&thread_queue));
                loop {
                    let next = {
                        let (lock, cvar) = &*thread_queue;
                        let mut q = lock.lock().expect("worker queue mutex poisoned");
                        loop {
                            if let Some(item) = q.pop() {
                                break Some(item);
                            }
                            if q.stopped {
                                break None;
                            }
                            q = cvar.wait(q).expect("worker condvar poisoned");
                        }
                    };
                    let Some((priority, task)) = next else {
                        break;
                    };
                    if priority == WorkerPriority::Foreground
                        && thread_shutdown.load(Ordering::Acquire)
                    {
                        // Spec rule 7: once shutdown begins, queued priority-0 tasks become no-ops,
                        // leaving their callers' handles incomplete.
                        continue;
                    }
                    task(&mut core);
                }
                core.close_cache_swallowing_error();
            })?;

        Ok(Self {
            dir,
            queue,
            shutting_down,
            mode: ExecutionMode::Threaded {
                join_handle: Mutex::new(Some(join_handle)),
            },
        })
    }

    /// Directory managed by this worker.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Returns `true` if shutdown has been initiated.
    #[must_use]
    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Acquire)
    }

    fn enqueue(&self, priority: WorkerPriority, task: WorkerTask) {
        let (lock, cvar) = &*self.queue;
        let mut q = lock.lock().expect("worker queue mutex poisoned");
        q.push(priority, task);
        cvar.notify_one();
    }

    /// Executes a single highest-priority queued task in synchronous mode.
    ///
    /// Returns `true` if a task was executed, or `false` if all queues are empty (or if running
    /// in background-thread mode).
    pub fn step_one(&self) -> bool {
        let ExecutionMode::Sync(core_mutex) = &self.mode else {
            return false;
        };
        let next = {
            let (lock, _) = &*self.queue;
            let mut q = lock.lock().expect("worker queue mutex poisoned");
            q.pop()
        };
        let Some((priority, task)) = next else {
            return false;
        };
        if priority == WorkerPriority::Foreground && self.shutting_down.load(Ordering::Acquire) {
            return true;
        }
        let mut core = core_mutex.lock().expect("worker core mutex poisoned");
        task(&mut core);
        true
    }

    /// Executes all currently queued tasks until all priority queues are empty (synchronous mode).
    pub fn drain_sync(&self) {
        while self.step_one() {}
    }

    /// Returns the number of chunks currently buffered in the pending-write map (synchronous mode).
    #[must_use]
    pub fn pending_write_count_sync(&self) -> Option<usize> {
        let ExecutionMode::Sync(core_mutex) = &self.mode else {
            return None;
        };
        let core = core_mutex.lock().expect("worker core mutex poisoned");
        Some(core.pending_writes.len())
    }

    /// Submits a priority-`0` chunk store (`Some(tag)` to write, `None` to clear/delete).
    ///
    /// Evaluates `tag_supplier` at priority `0`, coalesces writes for an already-queued
    /// [`ChunkPos`] in place (preserving insertion order and returning the existing completion
    /// handle), and schedules a priority-`1` background drain step.
    pub fn store_with_supplier<F>(&self, pos: ChunkPos, tag_supplier: F) -> IoHandle<()>
    where
        F: FnOnce() -> Option<Compound> + Send + 'static,
    {
        let outer = IoHandle::new();
        let outer_clone = outer.clone();
        self.enqueue(
            WorkerPriority::Foreground,
            Box::new(move |core| {
                let tag = tag_supplier();
                let _ = core.pending_writes.upsert(pos, tag, Some(outer_clone));
                core.schedule_background_drain();
            }),
        );
        outer
    }

    /// Submits a priority-`0` store and resolves to the **underlying** pending-write completion
    /// handle (identical across coalesced writes for the same [`ChunkPos`]).
    pub fn submit_store_returning_handle(
        &self,
        pos: ChunkPos,
        tag: Option<Compound>,
    ) -> IoHandle<IoHandle<()>> {
        let handle_out = IoHandle::new();
        let handle_out_clone = handle_out.clone();
        self.enqueue(
            WorkerPriority::Foreground,
            Box::new(move |core| {
                let entry_handle = core.pending_writes.upsert(pos, tag, None);
                core.schedule_background_drain();
                handle_out_clone.complete(Ok(entry_handle));
            }),
        );
        handle_out
    }

    /// Convenience: submits a priority-`0` store for `pos` with `tag` (`None` deletes/clears).
    pub fn store(&self, pos: ChunkPos, tag: Option<Compound>) -> IoHandle<()> {
        self.store_with_supplier(pos, move || tag)
    }

    /// Submits a priority-`0` foreground read.
    ///
    /// If `pos` is in the pending-write map, returns a **deep copy** (`Compound::clone()`,
    /// matching vanilla `CompoundTag.copy()`) without touching disk, or `Ok(None)` if a
    /// deletion is pending. Otherwise reads from the region cache.
    pub fn load(&self, pos: ChunkPos) -> IoHandle<Option<Compound>> {
        let handle = IoHandle::new();
        let handle_clone = handle.clone();
        self.enqueue(
            WorkerPriority::Foreground,
            Box::new(move |core| {
                if let Some(pending) = core.pending_writes.get(pos) {
                    handle_clone.complete(Ok(pending.clone()));
                    return;
                }
                let res = core.cache.read(pos).map_err(Arc::new);
                handle_clone.complete(res);
            }),
        );
        handle
    }

    /// Submits a priority-`0` streaming scan.
    ///
    /// If `pos` is in the pending-write map, drives `visitor` over the in-memory tag (or skips
    /// if a deletion is pending); otherwise streams directly from disk with early termination.
    pub fn scan<V>(&self, pos: ChunkPos, mut visitor: V) -> IoHandle<(bool, V)>
    where
        V: StreamTagVisitor + Send + 'static,
    {
        let handle = IoHandle::new();
        let handle_clone = handle.clone();
        self.enqueue(
            WorkerPriority::Foreground,
            Box::new(move |core| {
                if let Some(pending) = core.pending_writes.get(pos) {
                    match pending {
                        Some(tag) => {
                            scan_compound_in_memory(tag, &mut visitor);
                            handle_clone.complete(Ok((true, visitor)));
                        }
                        None => {
                            handle_clone.complete(Ok((false, visitor)));
                        }
                    }
                    return;
                }
                match core.cache.scan(pos, &mut visitor) {
                    Ok(found) => handle_clone.complete(Ok((found, visitor))),
                    Err(err) => handle_clone.complete(Err(Arc::new(err))),
                }
            }),
        );
        handle
    }

    /// Submits a priority-`0` synchronize barrier.
    ///
    /// Captures all currently pending writes; once all of them complete on disk:
    /// - If `flush == true`: submits a priority-`0` task calling `RegionCache::flush()` (`fsync`
    ///   on all open `.mca` files).
    /// - If `flush == false`: submits a no-op priority-`0` barrier.
    pub fn synchronize(&self, flush: bool) -> IoHandle<()> {
        let handle = IoHandle::new();
        let handle_clone = handle.clone();
        self.enqueue(
            WorkerPriority::Foreground,
            Box::new(move |core| {
                let captured = core.pending_writes.snapshot_handles();
                if captured.is_empty() {
                    core.enqueue(
                        WorkerPriority::Foreground,
                        Box::new(move |core| {
                            let res = if flush {
                                core.cache.flush().map_err(Arc::new)
                            } else {
                                Ok(())
                            };
                            handle_clone.complete(res);
                        }),
                    );
                } else {
                    core.sync_waiters.push(SyncWaiter {
                        handles: captured,
                        flush,
                        result_handle: handle_clone,
                    });
                }
            }),
        );
        handle
    }

    /// Initiates idempotent shutdown (sets atomic flag and enqueues the priority-`2` drain barrier)
    /// without stepping synchronous queues. Useful for testing shutdown queue behavior step-by-step.
    pub fn begin_shutdown(&self) -> Option<IoHandle<()>> {
        if self.shutting_down.swap(true, Ordering::AcqRel) {
            return None;
        }
        let barrier = IoHandle::new();
        let barrier_clone = barrier.clone();
        self.enqueue(
            WorkerPriority::Shutdown,
            Box::new(move |core| {
                core.close_cache_swallowing_error();
                barrier_clone.complete(Ok(()));
            }),
        );
        Some(barrier)
    }

    /// Idempotently shuts down the worker:
    /// - Sets the atomic shutdown flag (queued priority-`0` tasks become no-ops).
    /// - Drains all already-buffered priority-`1` writes to disk.
    /// - Executes the priority-`2` shutdown barrier and closes the region cache (logging and
    ///   swallowing any close error at the worker boundary).
    /// - Joins the background thread if spawned.
    pub fn close(&self) {
        let barrier = self.begin_shutdown();
        match &self.mode {
            ExecutionMode::Sync(_) => {
                self.drain_sync();
                let (lock, _) = &*self.queue;
                lock.lock().expect("worker queue mutex poisoned").stopped = true;
            }
            ExecutionMode::Threaded { join_handle } => {
                if let Some(b) = barrier {
                    let _ = b.wait();
                }
                {
                    let (lock, cvar) = &*self.queue;
                    let mut q = lock.lock().expect("worker queue mutex poisoned");
                    q.stopped = true;
                    cvar.notify_all();
                }
                if let Some(jh) = join_handle
                    .lock()
                    .expect("join_handle mutex poisoned")
                    .take()
                {
                    let _ = jh.join();
                }
            }
        }
    }

    // --- Synchronous convenience helpers (work identically in Sync and Threaded modes) ---

    fn drive_handle<T: Clone>(
        &self,
        handle: &IoHandle<T>,
    ) -> std::result::Result<T, SharedStorageError> {
        match &self.mode {
            ExecutionMode::Sync(_) => {
                while !handle.is_complete() {
                    if !self.step_one() {
                        return Err(Arc::new(StorageError::WorkerShutdown));
                    }
                }
                handle
                    .try_get()
                    .expect("handle must be complete after loop")
            }
            ExecutionMode::Threaded { .. } => {
                if self.shutting_down.load(Ordering::Acquire) && !handle.is_complete() {
                    return Err(Arc::new(StorageError::WorkerShutdown));
                }
                handle.wait()
            }
        }
    }

    /// Synchronous convenience wrapper: stores `tag` for `pos` and waits until written to disk.
    pub fn store_sync(
        &self,
        pos: ChunkPos,
        tag: Option<Compound>,
    ) -> std::result::Result<(), SharedStorageError> {
        let h = self.store(pos, tag);
        self.drive_handle(&h)
    }

    /// Synchronous convenience wrapper: reads `pos` at priority `0` and returns the result.
    pub fn load_sync(
        &self,
        pos: ChunkPos,
    ) -> std::result::Result<Option<Compound>, SharedStorageError> {
        let h = self.load(pos);
        self.drive_handle(&h)
    }

    /// Synchronous convenience wrapper: streaming-scans `pos` at priority `0`.
    pub fn scan_sync<V>(
        &self,
        pos: ChunkPos,
        visitor: V,
    ) -> std::result::Result<(bool, V), SharedStorageError>
    where
        V: StreamTagVisitor + Send + 'static,
    {
        let handle = self.scan(pos, visitor);
        match &self.mode {
            ExecutionMode::Sync(_) => {
                while !handle.is_complete() {
                    if !self.step_one() {
                        return Err(Arc::new(StorageError::WorkerShutdown));
                    }
                }
                let mut guard = handle.inner.result.lock().expect("IoHandle mutex poisoned");
                guard.take().expect("handle must be complete after loop")
            }
            ExecutionMode::Threaded { .. } => {
                let mut guard = handle.inner.result.lock().expect("IoHandle mutex poisoned");
                while guard.is_none() {
                    guard = handle
                        .inner
                        .cvar
                        .wait(guard)
                        .expect("IoHandle condvar poisoned");
                }
                guard.take().expect("checked Some")
            }
        }
    }

    /// Synchronous convenience wrapper: flushes/synchronizes all currently pending writes.
    pub fn synchronize_sync(&self, flush: bool) -> std::result::Result<(), SharedStorageError> {
        let h = self.synchronize(flush);
        self.drive_handle(&h)
    }
}

impl Drop for StorageWorker {
    fn drop(&mut self) {
        self.close();
    }
}

// --- World blending old-chunk scanner ---

/// 1,024-bit per-region bitset indexed by `lz * 32 + lx` (`ChunkPos::header_index`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OldChunkBitset {
    bits: [u64; 16],
}

impl OldChunkBitset {
    /// Returns `true` if the bit at `index` (`0..1024`) is set.
    #[must_use]
    pub const fn get(&self, index: usize) -> bool {
        (self.bits[index / 64] & (1u64 << (index % 64))) != 0
    }

    /// Sets the bit at `index` (`0..1024`).
    pub fn set(&mut self, index: usize) {
        self.bits[index / 64] |= 1u64 << (index % 64);
    }

    /// Returns `true` if any bit is set in this region.
    #[must_use]
    pub fn any(&self) -> bool {
        self.bits.iter().any(|&w| w != 0)
    }
}

/// Streaming visitor collecting only top-level `Int("DataVersion")` and `Compound("blending_data")`,
/// halting early as soon as old-chunk status is proven.
#[derive(Debug, Default)]
pub struct OldChunkScanVisitor {
    depth: usize,
    reading_data_version: bool,
    data_version: Option<i32>,
    has_blending_data: bool,
}

impl OldChunkScanVisitor {
    /// Returns `true` iff `DataVersion` (defaulting to `0` if absent) `< 4997` OR `"blending_data"`
    /// is present.
    #[must_use]
    pub fn is_old_chunk(&self) -> bool {
        self.has_blending_data || self.data_version.unwrap_or(0) < BLENDER_OLD_CHUNK_VERSION_CUTOFF
    }
}

impl StreamTagVisitor for OldChunkScanVisitor {
    fn visit_root_entry(&mut self, tag_type: TagType) -> ValueResult {
        if tag_type == TagType::Compound {
            self.depth = 0;
            ValueResult::Continue
        } else {
            ValueResult::Halt
        }
    }

    fn visit_entry(&mut self, tag_type: TagType, name: &NbtString) -> EntryResult {
        if self.depth != 0 {
            return EntryResult::Skip;
        }
        if tag_type == TagType::Compound && name == "blending_data" {
            self.has_blending_data = true;
            return EntryResult::Halt;
        }
        if tag_type == TagType::Int && name == "DataVersion" {
            self.reading_data_version = true;
            return EntryResult::Enter;
        }
        EntryResult::Skip
    }

    fn visit_int(&mut self, value: i32) -> ValueResult {
        if self.reading_data_version {
            self.reading_data_version = false;
            self.data_version = Some(value);
            if value < BLENDER_OLD_CHUNK_VERSION_CUTOFF {
                return ValueResult::Halt;
            }
        }
        ValueResult::Continue
    }
}

/// 1,024-entry LRU cache of per-region old-chunk bitsets for world blending checks.
#[derive(Debug)]
pub struct OldChunkScanner {
    capacity: usize,
    entries: Vec<(u64, OldChunkBitset)>,
}

impl Default for OldChunkScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl OldChunkScanner {
    /// Creates a scanner with the standard 1,024-region LRU capacity.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(BLENDER_SCAN_CACHE_CAPACITY)
    }

    /// Creates a scanner with a custom LRU capacity (minimum `1`).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: Vec::new(),
        }
    }

    /// Number of currently cached region bitsets.
    #[must_use]
    pub fn cache_len(&self) -> usize {
        self.entries.len()
    }

    /// Returns the cached bitset for `region`, scanning all 1,024 slots via `worker` on cache miss.
    pub fn get_or_scan_region(
        &mut self,
        worker: &StorageWorker,
        region: RegionPos,
    ) -> &OldChunkBitset {
        let key = region.pack_key();
        if let Some(idx) = self.entries.iter().position(|(k, _)| *k == key) {
            let item = self.entries.remove(idx);
            self.entries.insert(0, item);
            return &self.entries[0].1;
        }

        let mut bitset = OldChunkBitset::default();
        for lz in 0..32u8 {
            for lx in 0..32u8 {
                let pos = region.chunk_pos(lx, lz);
                match worker.scan_sync(pos, OldChunkScanVisitor::default()) {
                    Ok((true, visitor)) => {
                        if visitor.is_old_chunk() {
                            bitset.set(pos.header_index());
                        }
                    }
                    Ok((false, _)) => {}
                    Err(err) => {
                        log::warn!(
                            "Blending scan failed for chunk ({}, {}): {err}; leaving bit clear",
                            pos.x,
                            pos.z
                        );
                    }
                }
            }
        }

        if self.entries.len() >= self.capacity {
            self.entries.pop();
        }
        self.entries.insert(0, (key, bitset));
        &self.entries[0].1
    }

    /// Returns `true` iff any chunk in the Chebyshev square
    /// `[center.x - range ..= center.x + range] × [center.z - range ..= center.z + range]`
    /// is classified as an old chunk (`DataVersion < 4997` or `"blending_data"` present).
    pub fn is_old_chunk_around(
        &mut self,
        worker: &StorageWorker,
        center: ChunkPos,
        range: i32,
    ) -> bool {
        if range < 0 {
            return false;
        }
        let min_x = center.x.saturating_sub(range);
        let max_x = center.x.saturating_add(range);
        let min_z = center.z.saturating_sub(range);
        let max_z = center.z.saturating_add(range);

        let min_rx = min_x >> 5;
        let max_rx = max_x >> 5;
        let min_rz = min_z >> 5;
        let max_rz = max_z >> 5;

        for rz in min_rz..=max_rz {
            for rx in min_rx..=max_rx {
                let region = RegionPos::new(rx, rz);
                let bitset = self.get_or_scan_region(worker, region);
                if !bitset.any() {
                    continue;
                }
                let start_x = min_x.max(rx << 5);
                let end_x = max_x.min((rx << 5) + 31);
                let start_z = min_z.max(rz << 5);
                let end_z = max_z.min((rz << 5) + 31);
                for z in start_z..=end_z {
                    for x in start_x..=end_x {
                        let pos = ChunkPos::new(x, z);
                        if bitset.get(pos.header_index()) {
                            return true;
                        }
                    }
                }
            }
        }

        false
    }
}
