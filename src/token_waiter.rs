use crate::waiter::Waiter;

use std::cell::Cell;
use std::fmt;
use std::io;
use std::marker::PhantomPinned;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

thread_local! {static TAG: Cell<usize> = const { Cell::new(0) }}

/// the id type from `TokenWaiter::get_id()`
#[derive(Debug)]
pub struct ID(NonZeroUsize);

impl ID {
    /// construct `ID` from `usize`
    ///
    /// # Safety
    ///
    /// the usize must be come from the previous `ID` instance
    pub unsafe fn from_usize(id: NonZeroUsize) -> Self {
        ID(id)
    }
}

impl From<ID> for usize {
    fn from(id: ID) -> Self {
        id.0.get()
    }
}

/// get id error
#[derive(Debug)]
pub struct Error;

/// token waiter that could be used for primitive wait blocking
pub struct TokenWaiter<T> {
    waiter: Waiter<T>,
    key: AtomicUsize,
    _phantom: PhantomPinned,
}

impl<T> TokenWaiter<T> {
    pub fn new() -> Self {
        TokenWaiter {
            key: AtomicUsize::new(0),
            waiter: Waiter::new(),
            _phantom: PhantomPinned,
        }
    }

    /// get the id of this token_waiter
    /// if the waiter is not triggered, we can't get id again
    pub fn id(&self) -> Result<ID, Error> {
        let id = self.key.load(Ordering::Relaxed);
        if id != 0 {
            // the id is already initialized
            return Err(Error);
        }

        // pin address is never changed
        let address = self as *const _ as usize;
        let tag = TAG.with(|t| {
            let x = t.get();
            t.set(x + 1);
            (x & 0x1f) << 1
        });

        let id = (address << 3) | tag;
        self.key.store(id, Ordering::Relaxed);
        Ok(ID(NonZeroUsize::new(id).unwrap()))
    }

    // make sure the id valid one from get id
    fn from_id(id: &ID) -> Option<&Self> {
        let id = id.0.get();
        // TODO: how to check if the address is valid?
        // if the id is wrong enough we could get a SIGSEGV
        let address = (id >> 3) & !0x7;
        let waiter = unsafe { &*(address as *const Self) };
        // need to check if the memory is still valid
        // lock the key to protect contention with drop
        if waiter
            .key
            .compare_exchange(id, id + 1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            Some(waiter)
        } else {
            None
        }
    }

    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T> {
        self.waiter.wait_rsp(timeout)
    }

    /// set rsp for the waiter with id
    /// the `id` must be come from `get_id()`
    pub fn set_rsp(id: ID, rsp: T) {
        if let Some(waiter) = Self::from_id(&id) {
            // clear the id so that we can get the id again
            waiter.key.store(0, Ordering::Release);
            // wake up the blocker
            waiter.waiter.set_rsp(rsp);
        }
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

// this is not necessary, we safely drop a non triggered token waiter
// impl<T> Drop for TokenWaiter<T> {
//     fn drop(&mut self) {
//         // wait for the key locked and clear it
//         let mut key = self.key.load(Ordering::Relaxed) & !1;
//         while let Err(v) =
//             self.key
//                 .compare_exchange_weak(key, 0, Ordering::AcqRel, Ordering::Relaxed)
//         {
//             key = v;
//             std::hint::spin_loop()
//         }
//     }
// }

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
                go!(move || TokenWaiter::set_rsp(id, j + 100));
                // this will block until the rsp was set
                assert_eq!(waiter.wait_rsp(None).unwrap(), j + 100);
                // after wait we can get the id again
                let id = waiter.id().unwrap();
                go!(move || TokenWaiter::set_rsp(id, j));
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
                TokenWaiter::set_rsp(id, 42)
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
}
