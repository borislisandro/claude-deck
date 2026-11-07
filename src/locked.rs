//! A mutex that survives a panic elsewhere. The data behind the daemon's locks is
//! plain state that is valid between statements, so a task that panicked while holding
//! one must not make every later `lock().unwrap()` panic too, which would leave the
//! daemon alive and deaf.

use std::sync::{Mutex, MutexGuard};

pub trait Locked<T> {
    fn locked(&self) -> MutexGuard<'_, T>;
}

impl<T> Locked<T> for Mutex<T> {
    fn locked(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_does_not_poison_the_state() {
        let m = std::sync::Arc::new(Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.locked();
            panic!("boom");
        })
        .join();
        assert!(m.lock().is_err(), "std would now refuse");
        *m.locked() += 1;
        assert_eq!(*m.locked(), 2);
    }
}
