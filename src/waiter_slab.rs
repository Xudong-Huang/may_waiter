use sharded_slab::Slab;

use crate::waiter::Waiter;

use std::io;
use std::sync::Arc;
use std::time::Duration;

pub struct SlabWaiter<T> {
    slab: Arc<WaiterSlab<T>>,
    entry: usize,
}

impl<T> SlabWaiter<T> {
    /// wait for response
    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T> {
        self.slab.wait_rsp(self.entry, timeout.into())
    }

    /// set rsp for the waiter
    pub fn set_rsp(&self, rsp: T) -> Result<(), T> {
        self.slab.set_rsp(self.entry, rsp)
    }

    /// get the id
    pub fn id(&self) -> usize {
        self.entry
    }
}

impl<T> Drop for SlabWaiter<T> {
    fn drop(&mut self) {
        // remove the entry
        self.slab.del_waiter(self.entry);
    }
}

/// Water guard to wait the response
#[derive(Debug)]
pub struct SlabWaiterGuard<'a, T: 'a> {
    owner: &'a WaiterSlab<T>,
    entry: usize,
}

impl<'a, T> SlabWaiterGuard<'a, T> {
    /// wait for response
    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T> {
        self.owner.wait_rsp(self.entry, timeout.into())
    }

    /// get the id
    pub fn id(&self) -> usize {
        self.entry
    }
}

impl<'a, T> Drop for SlabWaiterGuard<'a, T> {
    fn drop(&mut self) {
        // remove the entry
        self.owner.del_waiter(self.entry);
    }
}

/// Waiter map that could be used to wait response for given keys
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

    /// return a waiter on the stack!
    pub fn new_waiter(&self) -> SlabWaiterGuard<T> {
        let entry = self.slab.insert(Waiter::new()).expect("no slot available");
        SlabWaiterGuard { owner: self, entry }
    }

    /// return a waiter on the stack!
    pub fn make_waiter(self: &Arc<Self>) -> SlabWaiter<T> {
        let entry = self.slab.insert(Waiter::new()).expect("no slot available");
        SlabWaiter {
            slab: self.clone(),
            entry,
        }
    }

    // used internally
    fn del_waiter(&self, id: usize) {
        self.slab.remove(id);
    }

    fn wait_rsp(&self, id: usize, timeout: Option<Duration>) -> io::Result<T> {
        let waiter = self.slab.get(id).expect("can't find id in waiter slab");
        waiter.wait_rsp(timeout)
    }

    /// set rsp for the corresponding waiter
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
        let req_map = Arc::new(WaiterSlab::<usize>::new());
        let req_map_1 = req_map.clone();

        // one coroutine wait data send from another coroutine
        // prepare the waiter first
        let waiter = req_map.new_waiter();
        let id = waiter.id();

        // trigger the rsp in another coroutine
        go!(move || req_map_1.set_rsp(id, 100).ok());

        // this will block until the rsp was set
        let result = waiter.wait_rsp(None).unwrap();
        assert_eq!(result, 100);
    }

    #[test]
    fn test_slab_waiter() {
        use std::sync::Arc;
        let req_map = Arc::new(WaiterSlab::<usize>::new());

        // one coroutine wait data send from another coroutine
        // prepare the waiter first
        let waiter = Arc::new(req_map.make_waiter());
        let waiter_1 = waiter.clone();

        // trigger the rsp in another coroutine
        go!(move || { waiter_1.set_rsp(100).ok() });

        // this will block until the rsp was set
        let result = waiter.wait_rsp(None).unwrap();
        assert_eq!(result, 100);
    }
}
