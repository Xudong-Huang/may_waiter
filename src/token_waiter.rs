use crate::waiter::Waiter;

use sharded_slab::Slab;

use std::any::Any;
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

type RegistryValue = Arc<dyn Any + Send + Sync>;

// process-global registry that maps serialized ids to waiter state
fn registry() -> &'static Slab<RegistryValue> {
    static REGISTRY: OnceLock<Slab<RegistryValue>> = OnceLock::new();
    REGISTRY.get_or_init(Slab::new)
}

/// The waiter could not issue a new id because the previous id has not been
/// consumed by [`TokenWaiter::set_rsp`] yet.
#[derive(Debug)]
pub struct Error;

struct Inner<T> {
    // whether an id is outstanding; while true, `key` is its registry key
    outstanding: AtomicBool,
    key: AtomicUsize,
    waiter: Waiter<T>,
}

/// An id issued by [`TokenWaiter::id`].
///
/// The id is a plain `usize` token that can be serialized and reconstructed
/// with [`ID::from_usize`]. It is resolved through a process-global registry,
/// so it is only meaningful in the process that issued it; unknown or stale
/// ids are rejected by [`TokenWaiter::set_rsp`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ID(usize);

impl ID {
    /// Reconstruct an id from a `usize`, e.g. one received over the network.
    ///
    /// The id is validated when it is used with
    /// [`TokenWaiter::set_rsp`]; an unknown or stale id makes that call
    /// return `Err`.
    pub fn from_usize(id: usize) -> Self {
        ID(id)
    }
}

impl From<ID> for usize {
    fn from(id: ID) -> Self {
        id.0
    }
}

/// A token based waiter that needs no caller-managed map or slab.
///
/// The waiter stays with its owner (on the stack, in a `Box`, ...), while its
/// [`ID`] is handed to whoever produces the response. The id is a plain
/// `usize` token resolved through an internal process-global registry, so it
/// can be serialized (e.g. written to a network frame) and reconstructed with
/// [`ID::from_usize`], as long as it is used in the process that issued it.
/// `T` must be `Send + 'static` because the waiter state is shared with the
/// registry.
///
/// Only one id may be outstanding at a time; once [`TokenWaiter::set_rsp`]
/// accepts a response, a new id can be requested for the next round. Stale,
/// already consumed or wrongly typed ids are rejected and the response is
/// returned back.
pub struct TokenWaiter<T: Send + 'static> {
    inner: Arc<Inner<T>>,
}

impl<T: Send + 'static> TokenWaiter<T> {
    /// Create a new token waiter.
    pub fn new() -> Self {
        TokenWaiter {
            inner: Arc::new(Inner {
                outstanding: AtomicBool::new(false),
                key: AtomicUsize::new(0),
                waiter: Waiter::new(),
            }),
        }
    }

    /// Issue an id for this waiter.
    ///
    /// Returns [`Error`] if a previously issued id has not been consumed by
    /// [`TokenWaiter::set_rsp`] yet, since only one id may be outstanding
    /// at a time. After `set_rsp` accepts the response, a new id can be
    /// requested again.
    pub fn id(&self) -> Result<ID, Error> {
        // only one id may be outstanding at a time
        if self
            .inner
            .outstanding
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(Error);
        }

        // register the waiter state so that it can be found from the id
        let inner: Arc<Inner<T>> = Arc::clone(&self.inner);
        let value: RegistryValue = inner;
        let key = registry().insert(value).expect("no slot available");
        self.inner.key.store(key, Ordering::Relaxed);
        Ok(ID(key))
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
    /// The response is returned back in `Err` if the id is unknown or stale,
    /// has already been consumed, or was issued by a waiter with a different
    /// response type. The responder can thus learn that the response was not
    /// delivered.
    pub fn set_rsp(id: ID, rsp: T) -> Result<(), T> {
        let registry = registry();

        // look the entry up and check its type before claiming it, so that an
        // id of another response type cannot consume a live token
        let entry = match registry.get(id.0) {
            Some(entry) => entry,
            None => return Err(rsp),
        };
        let value = Arc::clone(&*entry);
        // `take` spins while the slot has outstanding references, so the guard
        // must be released before claiming the token
        drop(entry);

        let inner = match value.downcast::<Inner<T>>() {
            Ok(inner) => inner,
            Err(_) => return Err(rsp),
        };

        // claim the token, only one caller can win
        if registry.take(id.0).is_none() {
            return Err(rsp);
        }

        // release the token before delivering the rsp so that `id()` can be
        // called again right after `wait_rsp` returns
        inner.outstanding.store(false, Ordering::Release);
        inner.waiter.set_rsp(rsp);
        Ok(())
    }
}

impl<T: Send + 'static> Default for TokenWaiter<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + 'static> fmt::Debug for TokenWaiter<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "TokenWaiter{{ ... }}")
    }
}

impl<T: Send + 'static> Drop for TokenWaiter<T> {
    fn drop(&mut self) {
        // release a registry entry that was never claimed
        if self.inner.outstanding.load(Ordering::Acquire) {
            registry().remove(self.inner.key.load(Ordering::Relaxed));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use may::go;

    #[test]
    fn token_waiter_round_trip() {
        let waiter = TokenWaiter::<usize>::new();
        let raw: usize = waiter.id().unwrap().into();

        // the id is serialized and reconstructed on the responder side
        go!(move || {
            let id = ID::from_usize(raw);
            TokenWaiter::<usize>::set_rsp(id, 100).unwrap();
        });

        assert_eq!(waiter.wait_rsp(None).unwrap(), 100);

        // the same id cannot be redeemed twice
        let id = ID::from_usize(raw);
        assert_eq!(TokenWaiter::<usize>::set_rsp(id, 200), Err(200));

        // a new id can be issued for the next round
        let raw: usize = waiter.id().unwrap().into();
        go!(move || {
            let id = ID::from_usize(raw);
            TokenWaiter::<usize>::set_rsp(id, 300).unwrap();
        });
        assert_eq!(waiter.wait_rsp(None).unwrap(), 300);
    }

    #[test]
    fn token_waiter_stale_id() {
        let waiter = TokenWaiter::<usize>::new();
        let raw: usize = waiter.id().unwrap().into();
        drop(waiter);

        // using the id after the waiter is gone is safe and rejected
        let id = ID::from_usize(raw);
        assert_eq!(TokenWaiter::<usize>::set_rsp(id, 42), Err(42));
    }

    #[test]
    fn token_waiter_wrong_type() {
        let waiter = TokenWaiter::<usize>::new();
        let raw: usize = waiter.id().unwrap().into();

        // a response of the wrong type must not consume the token
        let res = TokenWaiter::<String>::set_rsp(ID::from_usize(raw), "wrong".into());
        assert!(res.is_err());

        go!(move || {
            let id = ID::from_usize(raw);
            TokenWaiter::<usize>::set_rsp(id, 7).unwrap();
        });
        assert_eq!(waiter.wait_rsp(None).unwrap(), 7);
    }

    #[test]
    fn token_waiter_one_id_at_a_time() {
        let waiter = TokenWaiter::<usize>::new();
        assert!(waiter.id().is_ok());
        assert!(waiter.id().is_err());
    }

    #[test]
    fn token_waiter_moved() {
        let waiter = TokenWaiter::<usize>::new();
        let raw: usize = waiter.id().unwrap().into();

        // the id is a registry key, not an address, so moving the waiter is
        // harmless and it does not need to be pinned
        let waiter = Box::new(waiter);
        go!(move || {
            let id = ID::from_usize(raw);
            TokenWaiter::<usize>::set_rsp(id, 100).unwrap();
        });
        assert_eq!(waiter.wait_rsp(None).unwrap(), 100);
    }

    #[test]
    fn token_waiter_timeout() {
        let result = go!(|| {
            let waiter = TokenWaiter::<usize>::new();
            let raw: usize = waiter.id().unwrap().into();

            // trigger the rsp after the timeout expires
            let h = go!(move || {
                may::coroutine::sleep(Duration::from_millis(102));
                TokenWaiter::<usize>::set_rsp(ID::from_usize(raw), 42).ok()
            });

            let ret = waiter.wait_rsp(Duration::from_millis(100));
            h.join().unwrap();
            ret
        })
        .join()
        .unwrap();

        assert!(result.is_err());
    }
}
