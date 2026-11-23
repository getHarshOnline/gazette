//! Scoped serving of the retained generation during bounded native rebuild work.

use super::Error;
use gazette_core::{preview, publication::Key};
use gazette_ports::preview::{PreviewReservation, PreviewSession};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

/// Stop the scoped server even if a caller-supplied rebuild panics.
struct Stop<'a>(&'a AtomicBool);

impl Drop for Stop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Keep immutable output available without moving compilers or their local caches.
/// The worker is joined before its output borrow or listener ownership can change.
pub(super) fn during<R, T>(
    reserved: R,
    output: BTreeMap<Key, preview::Site<'_>>,
    work: impl FnOnce() -> T,
) -> Result<(R, T), Error>
where
    R: PreviewReservation + Send,
{
    let stop = AtomicBool::new(false);
    thread::scope(|scope| {
        let stopped = &stop;
        let worker = thread::Builder::new()
            .name("gazette-preview".into())
            .spawn_scoped(scope, move || {
                let mut session = reserved.attach(output)?;
                while !stopped.load(Ordering::Acquire) {
                    match session.poll()? {
                        preview::Poll::Idle => thread::sleep(Duration::from_millis(10)),
                        preview::Poll::Responded | preview::Poll::Disconnected => (),
                    }
                }
                Ok::<R, preview::Error>(session.detach())
            })
            .map_err(|error| Error::Transport(preview::Error::Io(error)))?;
        let guard = Stop(&stop);
        let result = work();
        drop(guard);
        let reservation = match worker.join() {
            Ok(result) => result.map_err(Error::Transport)?,
            Err(panic) => std::panic::resume_unwind(panic),
        };
        Ok((reservation, result))
    })
}
