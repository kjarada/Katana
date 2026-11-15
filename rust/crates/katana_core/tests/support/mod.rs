use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static DIRECTORY_LOCK: Mutex<()> = Mutex::new(());

pub struct Scratch {
    pub root: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("{name}_{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    pub fn touch(&self, relative: impl AsRef<Path>) -> PathBuf {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "x").unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub struct WorkingDirectory {
    before: PathBuf,
    _lock: MutexGuard<'static, ()>,
}

impl WorkingDirectory {
    pub fn enter(path: &Path) -> Self {
        let lock = DIRECTORY_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let before = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self {
            before,
            _lock: lock,
        }
    }
}

impl Drop for WorkingDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.before).unwrap();
    }
}
