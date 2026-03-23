/// Extension trait for converting [`rustix::io::Errno`] into [`color_eyre::eyre::Report`].
///
/// Eliminates the `.map_err(|e| eyre!(e))` boilerplate at every rustix call site.
use color_eyre::eyre;
use rustix::io::Errno;

pub trait ErrnoExt<T> {
    fn into_eyre(self) -> eyre::Result<T>;
}

impl<T> ErrnoExt<T> for Result<T, Errno> {
    fn into_eyre(self) -> eyre::Result<T> { self.map_err(|e| eyre::eyre!(e)) }
}
