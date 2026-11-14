//! Identical reads asked at once go to Proton once.
//!
//! Opening a mail, reading the next ones ahead and preparing a picture of it can all ask for the same conversation in
//! the same moment, and each cache answers only once a read has come back. So the first read of a path goes, and the
//! others asked while it is out wait for its answer instead of asking again. Nothing is kept past that: the next read
//! after the answer goes again. A change makes every read still out old, so it is forgotten, and a read asked after
//! the change goes on its own.
use crate::error::{Error, Result};
use parking_lot::Mutex;
use std::{collections::HashMap, future::Future, sync::Arc};
use tokio::sync::OnceCell;

type Read<T> = Arc<OnceCell<Result<T>>>;

pub struct Together<T> {
    out: Mutex<HashMap<String, Read<T>>>,
}

impl<T> Default for Together<T> {
    fn default() -> Self {
        Self { out: Mutex::new(HashMap::new()) }
    }
}

impl<T: Clone + Send + Sync> Together<T> {
    /// The answer to `key`: `asking`'s own, or that of the same read already out. If the one asking is dropped,
    /// another waiting takes the read over.
    pub async fn ask<F>(&self, key: &str, asking: impl FnOnce() -> F + Send) -> Result<T>
    where
        F: Future<Output = Result<T>> + Send,
    {
        let read = self.out.lock().entry(key.to_string()).or_default().clone();
        let waiting = Waiting { together: self, key, read: Some(read) };
        let Some(read) = waiting.read.as_ref() else { return Err(Error::Cancelled) };
        read.get_or_init(asking).await;
        waiting.answer()
    }

    /// After a change: a read still out may hold what came before it.
    pub fn forget(&self) {
        self.out.lock().clear();
    }

    /// Reads out now, for tests.
    #[cfg(test)]
    pub fn out_now(&self) -> usize {
        self.out.lock().len()
    }
}

/// One who asked. The last to leave a read, answered or not, takes it out of the list, so that a dropped read is not
/// left in it.
struct Waiting<'a, T> {
    together: &'a Together<T>,
    key: &'a str,
    read: Option<Read<T>>,
}

impl<T> Waiting<'_, T> {
    /// Takes the read out of the list if no one else waits on it.
    fn leave(&self, read: &Read<T>) {
        let mut out = self.together.out.lock();
        // The list holds one, and this waiter the other: anyone else joins under the same lock.
        if out.get(self.key).is_some_and(|listed| Arc::ptr_eq(listed, read) && Arc::strong_count(read) == 2) {
            out.remove(self.key);
        }
    }
}

impl<T: Clone> Waiting<'_, T> {
    /// The answer, kept whole by the last one waiting and copied for the others.
    fn answer(mut self) -> Result<T> {
        let Some(read) = self.read.take() else { return Err(Error::Cancelled) };
        self.leave(&read);
        match Arc::try_unwrap(read) {
            Ok(alone) => alone.into_inner().unwrap_or(Err(Error::Cancelled)),
            Err(shared) => shared.get().cloned().unwrap_or(Err(Error::Cancelled)),
        }
    }
}

impl<T> Drop for Waiting<'_, T> {
    fn drop(&mut self) {
        if let Some(read) = self.read.take() {
            self.leave(&read);
        }
    }
}

#[cfg(test)]
mod tests;
