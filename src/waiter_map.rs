use dashmap::DashMap;

use crate::waiter::Waiter;

use std::fmt::{self, Debug};
use std::hash::Hash;
use std::io;
use std::time::Duration;

/// Water guard to wait the response
#[derive(Debug)]
pub struct WaiterGuard<'a, K: Hash + Eq + 'a, T: 'a> {
    owner: &'a WaiterMap<K, T>,
    id: K,
}

impl<'a, K: Hash + Eq + Debug, T> WaiterGuard<'a, K, T> {
    /// wait for response
    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T> {
        self.owner.wait_rsp(&self.id, timeout.into())
    }
}

impl<'a, K: Hash + Eq, T> Drop for WaiterGuard<'a, K, T> {
    fn drop(&mut self) {
        // remove the entry
        self.owner.del_waiter(&self.id);
    }
}

/// Waiter map that could be used to wait response for given keys
pub struct WaiterMap<K, T> {
    map: DashMap<K, Waiter<T>>,
}

impl<K: Hash + Eq, T> Debug for WaiterMap<K, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "WaiterMap{{ ... }}")
    }
}

impl<K: Hash + Eq, T> Default for WaiterMap<K, T> {
    fn default() -> Self {
        WaiterMap::new()
    }
}

impl<K: Hash + Eq, T> WaiterMap<K, T> {
    pub fn new() -> Self {
        WaiterMap {
            map: DashMap::new(),
        }
    }

    /// return a waiter on the stack!
    pub fn new_waiter(&self, id: K) -> WaiterGuard<K, T>
    where
        K: Clone,
    {
        // if we add a same key, the old waiter would be lost!
        match self.map.insert(id.clone(), Waiter::new()) {
            Some(_w) => panic!("waiter id already in use!"),
            None => WaiterGuard { owner: self, id },
        }
    }

    fn del_waiter(&self, id: &K) -> Option<Waiter<T>> {
        self.map.remove(id).map(|v| v.1)
    }

    fn wait_rsp(&self, id: &K, timeout: Option<Duration>) -> io::Result<T>
    where
        K: Debug,
    {
        fn extend_lifetime<'a, T>(r: &T) -> &'a T {
            unsafe { ::std::mem::transmute(r) }
        }

        let waiter = match self.map.get(id) {
            // extends the lifetime of the waiter ref
            Some(v) => extend_lifetime(&*v),
            None => unreachable!("can't find id in waiter map!"),
        };

        waiter.wait_rsp(timeout)
    }

    /// set rsp for the corresponding waiter
    pub fn set_rsp(&self, id: &K, rsp: T) -> Result<(), T>
    where
        K: Debug,
    {
        match self.map.get(id) {
            Some(waiter) => {
                waiter.set_rsp(rsp);
                Ok(())
            }
            None => Err(rsp),
        }
    }

    /// cancel all the waiting waiter, all wait would return NotFound error
    pub fn cancel_all(&mut self) {
        self.map.iter().for_each(|waiter| waiter.cancel_wait());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use may::go;

    #[test]
    fn test_waiter_map() {
        use std::sync::Arc;
        let req_map = Arc::new(WaiterMap::<usize, usize>::new());
        let req_map_1 = req_map.clone();

        let key = 1234;

        // one coroutine wait data send from another coroutine
        // prepare the waiter first
        let waiter = req_map.new_waiter(key);

        // trigger the rsp in another coroutine
        go!(move || req_map_1.set_rsp(&key, 100).ok());

        // this will block until the rsp was set
        let result = waiter.wait_rsp(None).unwrap();
        assert_eq!(result, 100);
    }
}
