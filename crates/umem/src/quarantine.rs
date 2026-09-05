//! Permanent storage for mappings whose device completion is uncertain.

use std::sync::{Arc, Mutex, OnceLock};

use crate::Backing;

fn entries() -> &'static Mutex<Vec<Arc<Backing>>> {
    static ENTRIES: OnceLock<Mutex<Vec<Arc<Backing>>>> = OnceLock::new();
    ENTRIES.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn retain(backing: Arc<Backing>) {
    entries()
        .lock()
        .expect("quarantine mutex poisoned")
        .push(backing);
}

#[cfg(test)]
pub(crate) fn len() -> usize {
    entries().lock().expect("quarantine mutex poisoned").len()
}
