use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use notify_debouncer_mini::notify::RecommendedWatcher;
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult, Debouncer};
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

pub struct FileWatcher {
    _debouncer: Debouncer<RecommendedWatcher>,
}

fn hash_file(path: &Path) -> Option<(u64, SystemTime)> {
    let metadata = fs::metadata(path).ok()?;
    let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let bytes = fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some((hasher.finish(), mtime))
}

impl FileWatcher {
    pub fn new<P: AsRef<Path>>(
        target_file: P,
        debounce_duration: Duration,
        sender: Sender<PathBuf>,
    ) -> Result<Self> {
        let target_path = target_file.as_ref().to_path_buf();
        let canonical_target = target_path
            .canonicalize()
            .unwrap_or_else(|_| target_path.clone());
        let _target_filename = canonical_target
            .file_name()
            .context("Invalid target file path")?
            .to_os_string();

        let watch_dir = canonical_target
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        let initial_state = hash_file(&canonical_target);
        let last_state = Arc::new(Mutex::new(initial_state));
        let last_state_clone = Arc::clone(&last_state);
        let target_clone = canonical_target.clone();

        let mut debouncer = new_debouncer(debounce_duration, move |res: DebounceEventResult| {
            if let Ok(events) = res {
                if events.is_empty() {
                    return;
                }
                // Verify if the target file content actually changed
                if let Some((new_hash, new_mtime)) = hash_file(&target_clone) {
                    if let Ok(mut lock) = last_state_clone.lock() {
                        let changed = match *lock {
                            Some((old_hash, _)) => new_hash != old_hash,
                            None => true,
                        };
                        if changed {
                            *lock = Some((new_hash, new_mtime));
                            let _ = sender.send(target_clone.clone());
                        }
                    }
                }
            }
        })
        .context("Failed to initialize debounced file watcher")?;

        debouncer
            .watcher()
            .watch(&watch_dir, RecursiveMode::NonRecursive)
            .context("Failed to attach watcher to target directory")?;

        Ok(Self {
            _debouncer: debouncer,
        })
    }
}
