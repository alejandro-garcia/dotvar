//! Non-recursive filesystem notifications with bounded, event-driven coalescing.
//!
//! Only paths are retained. There is no timer while idle, document I/O or secret
//! content in this module. Construct/reconcile on a background executor because
//! registering native watches and canonicalizing roots can perform filesystem I/O.

use crate::i18n::tr;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const COALESCE: Duration = Duration::from_millis(100);
const MAX_PATHS: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchBatch {
    /// Changes after reconcile carry the new generation. Ignore stale batches.
    pub generation: u64,
    pub paths: Vec<PathBuf>,
    /// Registration, native overflow/error or the path budget needs a scoped rescan.
    pub rescan: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WatchError;

impl std::fmt::Display for WatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&tr!("watch.unavailable"))
    }
}
impl std::error::Error for WatchError {}

#[derive(Default)]
struct Pending {
    generation: u64,
    paths: BTreeSet<PathBuf>,
    rescan: bool,
    stopped: bool,
}

/// A single native watcher and sleeping coalescer. Dropping releases both.
pub struct WatchService {
    watcher: Option<RecommendedWatcher>,
    roots: BTreeSet<PathBuf>,
    pending: Arc<Mutex<Pending>>,
    wake: mpsc::SyncSender<()>,
    worker: Option<JoinHandle<()>>,
    generation: u64,
}

impl WatchService {
    /// The callback runs on the coalescer thread. It should enqueue work only.
    pub fn new(callback: impl Fn(WatchBatch) + Send + 'static) -> Result<Self, WatchError> {
        let pending = Arc::new(Mutex::new(Pending::default()));
        let (wake, receiver) = mpsc::sync_channel(1);
        let work = pending.clone();
        let worker = thread::Builder::new()
            .name("env-filesystem-events".into())
            .spawn(move || coalesce(work, receiver, callback))
            .map_err(|_| WatchError)?;
        Ok(Self {
            watcher: None,
            roots: BTreeSet::new(),
            pending,
            wake,
            worker: Some(worker),
            generation: 0,
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn watched_roots(&self) -> &BTreeSet<PathBuf> {
        &self.roots
    }

    /// Replaces the watched set transactionally. Aliases and repetitions collapse.
    /// A failed registration retains the previous set, generation and watcher.
    /// Passing an empty iterator releases native handles without ending service.
    pub fn reconcile(
        &mut self,
        roots: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Result<(), WatchError> {
        let roots: BTreeSet<_> = roots
            .into_iter()
            .map(|root| {
                let path = root.as_ref().canonicalize().map_err(|_| WatchError)?;
                if !path.is_dir() {
                    return Err(WatchError);
                }
                Ok(path)
            })
            .collect::<Result<_, _>>()?;
        if roots == self.roots {
            return Ok(());
        }
        let generation = self.generation.checked_add(1).ok_or(WatchError)?;
        let watcher = if roots.is_empty() {
            None
        } else {
            let pending = self.pending.clone();
            let wake = self.wake.clone();
            let scoped_roots = roots.clone();
            let mut watcher = notify::recommended_watcher(move |event| {
                publish(&pending, &wake, generation, &scoped_roots, event);
            })
            .map_err(|_| WatchError)?;
            for root in &roots {
                watcher
                    .watch(root, RecursiveMode::NonRecursive)
                    .map_err(|_| WatchError)?;
            }
            Some(watcher)
        };
        {
            let mut pending = self.pending.lock().map_err(|_| WatchError)?;
            pending.generation = generation;
            pending.paths.clear();
            // Revalidate once after changing registration to cover changes in
            // the gap before this generation became active.
            pending.rescan = !roots.is_empty();
        }
        self.generation = generation;
        self.roots = roots;
        self.watcher = watcher;
        if !self.roots.is_empty() {
            let _ = self.wake.try_send(());
        }
        Ok(())
    }
}

impl Drop for WatchService {
    fn drop(&mut self) {
        // Stop callbacks and wake the blocked worker before joining. The bounded
        // wake queue cannot delay shutdown behind a backlog of filesystem events.
        if let Ok(mut pending) = self.pending.lock() {
            pending.stopped = true;
            pending.paths.clear();
        }
        let _ = self.wake.try_send(());
        self.watcher.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn in_scope(path: &Path, roots: &BTreeSet<PathBuf>) -> bool {
    roots.contains(path) || path.parent().is_some_and(|parent| roots.contains(parent))
}

fn publish(
    pending: &Mutex<Pending>,
    wake: &mpsc::SyncSender<()>,
    generation: u64,
    roots: &BTreeSet<PathBuf>,
    event: notify::Result<notify::Event>,
) {
    let Ok(mut pending) = pending.lock() else {
        return;
    };
    if pending.stopped || pending.generation != generation {
        return;
    }
    match event {
        Ok(event) => {
            // Reading a document must not recursively trigger another reload on
            // backends that also report opens/accesses.
            if matches!(event.kind, notify::EventKind::Access(_)) && !event.need_rescan() {
                return;
            }
            pending.rescan |= event.need_rescan();
            for path in event.paths {
                // Backends may report more than requested. Keep our own shallow
                // filter, without probing/reopening paths that may have vanished.
                if in_scope(&path, roots) {
                    if pending.paths.len() < MAX_PATHS {
                        pending.paths.insert(path);
                    } else {
                        pending.rescan = true;
                    }
                }
            }
        }
        Err(_) => pending.rescan = true,
    }
    if !pending.paths.is_empty() || pending.rescan {
        let _ = wake.try_send(());
    }
}

fn coalesce(
    pending: Arc<Mutex<Pending>>,
    receiver: mpsc::Receiver<()>,
    callback: impl Fn(WatchBatch),
) {
    while receiver.recv().is_ok() {
        let deadline = Instant::now() + COALESCE;
        loop {
            if pending.lock().map_or(true, |pending| pending.stopped) {
                return;
            }
            let Some(wait) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            match receiver.recv_timeout(wait) {
                Ok(()) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        let batch = {
            let Ok(mut pending) = pending.lock() else {
                return;
            };
            if pending.stopped {
                return;
            }
            if pending.paths.is_empty() && !pending.rescan {
                continue;
            }
            WatchBatch {
                generation: pending.generation,
                paths: std::mem::take(&mut pending.paths).into_iter().collect(),
                rescan: std::mem::take(&mut pending.rescan),
            }
        };
        callback(batch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::{Event, EventKind};
    use std::{fs, sync::mpsc::Receiver};
    use tempfile::tempdir;

    fn service() -> (WatchService, Receiver<WatchBatch>) {
        let (sender, receiver) = mpsc::channel();
        let service = WatchService::new(move |batch| {
            let _ = sender.send(batch);
        })
        .unwrap();
        (service, receiver)
    }

    fn await_path(receiver: &Receiver<WatchBatch>, expected: &Path) -> WatchBatch {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let batch = receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("native notification for the written fixture");
            if batch.paths.contains(&expected.to_path_buf()) {
                return batch;
            }
        }
    }

    #[test]
    fn aggregation_is_bounded_shallow_and_discards_obsolete_generations() {
        let roots = BTreeSet::from([PathBuf::from("/synthetic/project")]);
        let pending = Mutex::new(Pending {
            generation: 2,
            ..Pending::default()
        });
        let (wake, _) = mpsc::sync_channel(1);
        let old = Event::new(EventKind::Any).add_path("/synthetic/project/old.env".into());
        publish(&pending, &wake, 1, &roots, Ok(old));
        let nested = Event::new(EventKind::Any).add_path("/synthetic/project/nested/.env".into());
        publish(&pending, &wake, 2, &roots, Ok(nested));
        let access = Event::new(EventKind::Access(notify::event::AccessKind::Read))
            .add_path("/synthetic/project/.env".into());
        publish(&pending, &wake, 2, &roots, Ok(access));
        assert!(pending.lock().unwrap().paths.is_empty());
        for index in 0..MAX_PATHS + 20 {
            let path = PathBuf::from(format!("/synthetic/project/{index}.env"));
            let event = Event::new(EventKind::Any).add_path(path);
            publish(&pending, &wake, 2, &roots, Ok(event));
        }
        let pending = pending.lock().unwrap();
        assert_eq!(pending.paths.len(), MAX_PATHS);
        assert!(pending.rescan);
    }

    #[test]
    fn burst_notifications_are_coalesced_and_idle_does_not_emit_batches() {
        let (service, receiver) = service();
        let path = PathBuf::from("/synthetic/project/.env");
        let roots = BTreeSet::from([PathBuf::from("/synthetic/project")]);
        for _ in 0..20 {
            publish(
                &service.pending,
                &service.wake,
                service.generation(),
                &roots,
                Ok(Event::new(EventKind::Any).add_path(path.clone())),
            );
        }
        let batch = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(batch.paths, vec![path]);
        assert!(!batch.rescan);
        assert!(matches!(
            receiver.recv_timeout(COALESCE * 2),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        drop(service);
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn watches_real_writes_and_atomic_replacements_without_polling() {
        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let path = root.join(".env");
        fs::write(&path, b"SYNTHETIC=first\n").unwrap();
        let (mut service, receiver) = service();
        service.reconcile([&root]).unwrap();
        fs::write(&path, b"SYNTHETIC=second\n").unwrap();
        let batch = await_path(&receiver, &path);
        assert_eq!(batch.generation, service.generation());
        let replacement = root.join(".replacement");
        fs::write(&replacement, b"SYNTHETIC=third\n").unwrap();
        fs::rename(replacement, &path).unwrap();
        let batch = await_path(&receiver, &path);
        assert_eq!(batch.generation, service.generation());
    }

    #[test]
    fn reconcile_deduplicates_aliases_retains_previous_on_failure_and_releases_roots() {
        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let (mut service, _) = service();
        service.reconcile([root.clone(), root.join(".")]).unwrap();
        let generation = service.generation();
        assert_eq!(service.watched_roots(), &BTreeSet::from([root.clone()]));
        service.reconcile([&root]).unwrap();
        assert_eq!(service.generation(), generation);
        assert!(service.reconcile([root.join("missing")]).is_err());
        assert_eq!(service.generation(), generation);
        assert_eq!(service.watched_roots(), &BTreeSet::from([root]));
        service.reconcile(Vec::<PathBuf>::new()).unwrap();
        assert!(service.watched_roots().is_empty());
        assert!(service.watcher.is_none());
    }

    #[test]
    fn native_watch_ignores_deep_changes_and_shutdown_suppresses_callbacks() {
        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let nested = root.join("nested");
        fs::create_dir(&nested).unwrap();
        let path = nested.join("hidden.env");
        fs::write(&path, b"SYNTHETIC=first\n").unwrap();
        let (mut service, receiver) = service();
        service.reconcile([&root]).unwrap();
        fs::write(&path, b"SYNTHETIC=second\n").unwrap();
        let deadline = Instant::now() + Duration::from_millis(600);
        while let Ok(batch) =
            receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            assert!(!batch.paths.contains(&path));
        }
        drop(service);
        fs::write(&path, b"SYNTHETIC=third\n").unwrap();
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}
