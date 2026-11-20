use crate::RtFfiError;

///
/// A value that may only be used on the thread that created it, inside an object uniffi needs to be `Send + Sync`: a
/// three-d context, the preview, the GPU tracers. [cargo_uniffi_support::ThreadBound] checks the thread on every access
/// and drops a value released on another thread (Kotlin's cleaner) later on its own thread; this only maps its errors.
///
pub(crate) struct ThreadBound<T: 'static>(cargo_uniffi_support::ThreadBound<T>);

impl<T: 'static> ThreadBound<T> {
    pub fn new(value: T) -> Self {
        Self(cargo_uniffi_support::ThreadBound::new(value))
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, RtFfiError> {
        Ok(self.0.with_mut(f)?)
    }
}
