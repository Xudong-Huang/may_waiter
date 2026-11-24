use sharded_slab::Slab;

use crate::waiter::Waiter;

use std::io;
use std::sync::Arc;
use std::time::Duration;

/// Owned waiter guard. Removes the slab entry on drop.
pub struct SlabWaiterOwned<T> {
    slab: Arc<WaiterSlab<T>>,
    entry: usize,
}

impl<T> SlabWaiterOwned<T> {
    /// wait for response
    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T> {
        self.slab.wait_rsp(self.entry, timeout.into())
    }

    /// set rsp for the waiter
    pub fn set_rsp(&self, rsp: T) -> Result<(), T> {
        self.slab.set_rsp(self.entry, rsp)
    }

    /// Get the slab id, e.g. to hand it to the coroutine that produces the
    /// response.
    pub fn id(&self) -> usize {
        self.entry
    }
}

impl<T> Drop for SlabWaiterOwned<T> {
    fn drop(&mut self) {
        // remove the entry
        self.slab.del_waiter(self.entry);
    }
}

/// Borrowed waiter guard. Removes the slab entry on drop.
#[derive(Debug)]
pub struct SlabWaiter<'a, T: 'a> {
    owner: &'a WaiterSlab<T>,
    entry: usize,
}

impl<T> SlabWaiter<'_, T> {
    /// wait for response
    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T> {
        self.owner.wait_rsp(self.entry, timeout.into())
    }

    /// Get the slab id, e.g. to hand it to the coroutine that produces the
    /// response.
    pub fn id(&self) -> usize {
        self.entry
    }
}

impl<T> Drop for SlabWaiter<'_, T> {
    fn drop(&mut self) {
        // remove the entry
        self.owner.del_waiter(self.entry);
    }
}

/// A concurrent slab of waiters addressed by slab ids.
///
/// Slab ids carry a generation check, so a stale id (one whose waiter has been
/// removed) never resolves to a recycled slot: `set_rsp` returns the response
/// back in `Err` instead. Waiting does not hold a lock; `Slab::get` uses a
/// lock-free, ref-counted guard.
///
/// Note: usually you can use `Arc<Waiter>` directly.
pub struct WaiterSlab<T> {
    slab: Slab<Waiter<T>>,
}

impl<T> std::fmt::Debug for WaiterSlab<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "WaiterSlab{{ ... }}")
    }
}

impl<T> Default for WaiterSlab<T> {
    fn default() -> Self {
        WaiterSlab::new()
    }
}

impl<T> WaiterSlab<T> {
    pub fn new() -> Self {
        WaiterSlab { slab: Slab::new() }
    }

    /// Create a waiter and return a guard that removes the entry on drop.
    pub fn new_waiter(&self) -> SlabWaiter<'_, T> {
        let entry = self.slab.insert(Waiter::new()).expect("no slot available");
        SlabWaiter { owner: self, entry }
    }

    /// Create an owned waiter. The guard removes the entry on drop.
    ///
    /// As with `WaiterMap::new_waiter_owned`, a waiter created outside a
    /// coroutine must not be waited on from coroutine context.
    pub fn new_waiter_owned(self: &Arc<Self>) -> SlabWaiterOwned<T> {
        let entry = self.slab.insert(Waiter::new()).expect("no slot available");
        SlabWaiterOwned {
            slab: self.clone(),
            entry,
        }
    }

    // used internally
    fn del_waiter(&self, id: usize) {
        self.slab.remove(id);
    }

    fn wait_rsp(&self, id: usize, timeout: Option<Duration>) -> io::Result<T> {
        // `Slab::get` returns a lock-free, ref-counted guard, so holding it
        // across the wait does not block insert/remove/set_rsp operations
        let waiter = self.slab.get(id).expect("can't find id in waiter slab");
        waiter.wait_rsp(timeout)
    }

    /// Set the response for the waiter associated with `id`.
    ///
    /// Returns the response back in `Err` if the slab has no waiter for the
    /// id, e.g. because the waiter has already been dropped.
    pub fn set_rsp(&self, id: usize, rsp: T) -> Result<(), T> {
        match self.slab.get(id) {
            Some(waiter) => {
                waiter.set_rsp(rsp);
                Ok(())
            }
            None => Err(rsp),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use may::go;

    #[test]
    fn test_waiter_slab() {
        use std::sync::Arc;
        let req_slab = Arc::new(WaiterSlab::<usize>::new());
        let req_slab_1 = req_slab.clone();

        // one coroutine wait data send from another coroutine
        // prepare the waiter first
        let waiter = req_slab.new_waiter();
        let id = waiter.id();

        // trigger the rsp in another coroutine
        go!(move || req_slab_1.set_rsp(id, 100).ok());

        // this will block until the rsp was set
        let result = waiter.wait_rsp(None).unwrap();
        assert_eq!(result, 100);
    }

    #[test]
    fn test_slab_waiter() {
        use std::sync::Arc;
        let req_slab = Arc::new(WaiterSlab::<usize>::new());

        // one coroutine wait data send from another coroutine
        // prepare the waiter first
        let waiter = Arc::new(req_slab.new_waiter_owned());
        let waiter_1 = waiter.clone();

        // trigger the rsp in another coroutine
        go!(move || { waiter_1.set_rsp(100).ok() });

        // this will block until the rsp was set
        let result = waiter.wait_rsp(None).unwrap();
        assert_eq!(result, 100);
    }
}
