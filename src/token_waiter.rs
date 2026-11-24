use crate::waiter::Waiter;

use std::fmt;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// shared state of a `TokenWaiter`, kept alive by outstanding [`ID`]s
struct Inner<T> {
    // 0 if no id is outstanding, otherwise the generation of the id
    key: AtomicUsize,
    // source of the id generations, never 0
    gen: AtomicUsize,
    waiter: Waiter<T>,
}

/// An id issued by [`TokenWaiter::id`].
///
/// The id holds a handle to the waiter's shared state, so it can be sent to
/// another coroutine or thread and used with [`TokenWaiter::set_rsp`] even
/// after the owning [`TokenWaiter`] has been dropped. It can be redeemed at
/// most once; stale ids are rejected rather than delivered.
pub struct ID<T> {
    inner: Arc<Inner<T>>,
    gen: usize,
}

impl<T> fmt::Debug for ID<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ID{{ gen: {} }}", self.gen)
    }
}

/// The waiter could not issue a new id because the previous id has not been
/// consumed by [`TokenWaiter::set_rsp`] yet.
#[derive(Debug)]
pub struct Error;

/// A token based waiter that needs no shared map or slab.
///
/// The waiter stays with its owner (on the stack, in a `Box`, ...), while a
/// lightweight [`ID`] is handed to whoever produces the response. Only one id
/// may be outstanding at a time; once [`TokenWaiter::set_rsp`] accepts a
/// response, a new id can be requested for the next round.
pub struct TokenWaiter<T> {
    inner: Arc<Inner<T>>,
}

impl<T> TokenWaiter<T> {
    /// Create a new token waiter.
    pub fn new() -> Self {
        TokenWaiter {
            inner: Arc::new(Inner {
                key: AtomicUsize::new(0),
                gen: AtomicUsize::new(0),
                waiter: Waiter::new(),
            }),
        }
    }

    /// Issue an id for this waiter.
    ///
    /// Returns [`Error`] if a previously issued id has not been consumed by
    /// [`TokenWaiter::set_rsp`] yet, since only one id may be outstanding at a
    /// time. After `set_rsp` accepts the response, a new id can be requested
    /// again.
    pub fn id(&self) -> Result<ID<T>, Error> {
        let gen = self.next_gen();
        // only one id may be outstanding at a time, 0 means none
        match self
            .inner
            .key
            .compare_exchange(0, gen, Ordering::AcqRel, Ordering::Relaxed)
        {
            Ok(_) => Ok(ID {
                inner: Arc::clone(&self.inner),
                gen,
            }),
            Err(_) => Err(Error),
        }
    }

    // take a fresh generation, never returning 0 so that 0 can mean "no id"
    fn next_gen(&self) -> usize {
        let mut gen = self.inner.gen.load(Ordering::Relaxed);
        loop {
            let next = if gen == usize::MAX { 1 } else { gen + 1 };
            match self.inner.gen.compare_exchange_weak(
                gen,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return next,
                Err(actual) => gen = actual,
            }
        }
    }

    /// Block until the response is set by [`TokenWaiter::set_rsp`].
    ///
    /// Returns an error with kind [`io::ErrorKind::TimedOut`] if the timeout
    /// expires, and triggers the cancellation panic if the coroutine is
    /// cancelled while waiting.
    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T> {
        self.inner.waiter.wait_rsp(timeout)
    }

    /// Set the response for the waiter that issued `id`.
    ///
    /// The response is returned back in `Err` if the id has already been
    /// consumed, if it has been superseded by a newer id, or if the
    /// [`TokenWaiter`] itself has been dropped. The responder can thus learn
    /// that the response was not delivered.
    pub fn set_rsp(id: ID<T>, rsp: T) -> Result<(), T> {
        // the `id` holds one `Arc<Inner>`, if it is the only one left then the
        // `TokenWaiter` has already been dropped and nobody would wait for it
        if Arc::strong_count(&id.inner) == 1 {
            return Err(rsp);
        }
        // claim the outstanding id, a consumed or superseded id would fail
        if id
            .inner
            .key
            .compare_exchange(id.gen, 0, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return Err(rsp);
        }
        // the id is released before delivering the rsp so that `id()` can be
        // called again right after `wait_rsp` returns
        id.inner.waiter.set_rsp(rsp);
        Ok(())
    }
}

impl<T> fmt::Debug for TokenWaiter<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "TokenWaiter{{ ... }}")
    }
}

impl<T> Default for TokenWaiter<T> {
    fn default() -> Self {
        TokenWaiter::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use may::go;

    #[test]
    fn token_waiter_id() {
        let waiter = TokenWaiter::<usize>::new();
        assert!(waiter.id().is_ok());
        // the previous id should be consumed
        assert!(waiter.id().is_err());
    }

    #[test]
    fn token_waiter() {
        for j in 0..100 {
            let result = go!(move || {
                let waiter = TokenWaiter::<usize>::new();
                let id = waiter.id().unwrap();
                // trigger the rsp in another coroutine
                go!(move || TokenWaiter::set_rsp(id, j + 100).unwrap());
                // this will block until the rsp was set
                assert_eq!(waiter.wait_rsp(None).unwrap(), j + 100);
                // after wait we can get the id again
                let id = waiter.id().unwrap();
                go!(move || TokenWaiter::set_rsp(id, j).unwrap());
                waiter.wait_rsp(std::time::Duration::from_secs(2)).unwrap()
            })
            .join()
            .unwrap();

            assert_eq!(result, j);
        }
    }

    #[test]
    fn token_waiter_timeout() {
        let result = go!(|| {
            let waiter = TokenWaiter::<usize>::new();
            let id = waiter.id().unwrap();
            // trigger the rsp in another coroutine
            let h = go!(move || {
                may::coroutine::sleep(Duration::from_millis(102));
                TokenWaiter::set_rsp(id, 42).ok()
            });
            // this will block until the rsp was set
            let ret = waiter.wait_rsp(Duration::from_millis(100));
            h.join().unwrap();
            ret
        })
        .join()
        .unwrap();

        assert!(result.is_err());
    }

    #[test]
    fn token_waiter_stale_id() {
        let waiter = TokenWaiter::<usize>::new();
        let id = waiter.id().unwrap();
        // the waiter may be dropped while an id is still out there,
        // using the id afterwards must be safe and rejected
        drop(waiter);
        assert_eq!(TokenWaiter::set_rsp(id, 42), Err(42));
    }

    #[test]
    fn token_waiter_moved() {
        let waiter = TokenWaiter::<usize>::new();
        let id = waiter.id().unwrap();
        // moving the waiter no longer invalidates the id
        let waiter = Box::new(waiter);
        go!(move || TokenWaiter::set_rsp(id, 100).unwrap());
        assert_eq!(waiter.wait_rsp(None).unwrap(), 100);
    }
}
