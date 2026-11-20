use crate::RtFfiError;
use std::mem::ManuallyDrop;
use std::sync::Mutex;
use std::thread::ThreadId;

///
/// A value that may only be used on the thread that created it, inside an object uniffi needs to be `Send + Sync`.
/// A three-d context, the preview and the GPU tracers are such values: they are only touched through [ThreadBound::with],
/// which checks the thread, so they never actually move between threads.
///
pub(crate) struct ThreadBound<T> {
    owner: ThreadId,
    value: ManuallyDrop<Mutex<T>>,
}

// SAFETY: the value is only reached through `with`, which refuses every thread but the owner, and dropped only on the
// owner (see `Drop`).
unsafe impl<T> Send for ThreadBound<T> {}
unsafe impl<T> Sync for ThreadBound<T> {}

impl<T> ThreadBound<T> {
    pub fn new(value: T) -> Self {
        Self {
            owner: std::thread::current().id(),
            value: ManuallyDrop::new(Mutex::new(value)),
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, RtFfiError> {
        if std::thread::current().id() != self.owner {
            return Err(RtFfiError::WrongThread);
        }
        let mut guard = self
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(f(&mut guard))
    }
}

impl<T> Drop for ThreadBound<T> {
    fn drop(&mut self) {
        // A GPU device or an OpenGL context must not be released on another thread, for example on the finalizer
        // thread of Kotlin's garbage collector: then it is leaked. Close objects explicitly on their own thread.
        if std::thread::current().id() == self.owner {
            // SAFETY: dropped once, here
            unsafe { ManuallyDrop::drop(&mut self.value) };
        }
    }
}
