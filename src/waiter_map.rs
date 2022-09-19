use may::sync::Mutex;

use crate::waiter::Waiter;

use std::collections::HashMap;
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
    // TODO: use atomic hashmap instead
    map: Mutex<HashMap<K, Box<Waiter<T>>>>,
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

unsafe impl<K, T> Send for WaiterMap<K, T> {}
unsafe impl<K, T> Sync for WaiterMap<K, T> {}

impl<K: Hash + Eq, T> WaiterMap<K, T> {
    pub fn new() -> Self {
        WaiterMap {
            map: Mutex::new(HashMap::new()),
        }
    }

    /// return a waiter on the stack!
    pub fn new_waiter(&self, id: K) -> WaiterGuard<K, T>
    where
        K: Clone,
    {
        let mut m = self.map.lock().unwrap();
        // if we add a same key, the old waiter would be lost!
        m.insert(id.clone(), Box::new(Waiter::new()));
        WaiterGuard { owner: self, id }
    }

    // used internally
    fn del_waiter(&self, id: &K) -> Option<Box<Waiter<T>>> {
        let mut m = self.map.lock().unwrap();
        m.remove(id)
    }

    fn wait_rsp(&self, id: &K, timeout: Option<Duration>) -> io::Result<T>
    where
        K: Debug,
    {
        fn extend_lifetime<'a, T>(r: &T) -> &'a T {
            unsafe { ::std::mem::transmute(r) }
        }

        let map = self.map.lock().unwrap();
        let waiter = match map.get(id) {
            // extends the lifetime of the waiter ref
            Some(v) => extend_lifetime(v.as_ref()),
            None => unreachable!("can't find id in waiter map!"),
        };

        //release the mutex
        drop(map);

        waiter.wait_rsp(timeout)
    }

    /// set rsp for the corresponding waiter
    pub fn set_rsp(&self, id: &K, rsp: T) -> Result<(), T>
    where
        K: Debug,
    {
        let m = self.map.lock().unwrap();
        match m.get(id) {
            Some(waiter) => {
                waiter.set_rsp(rsp);
                Ok(())
            }
            None => Err(rsp),
        }
    }

    /// cancel all the waiting waiter, all wait would return NotFound error
    pub fn cancel_all(&self) {
        let m = self.map.lock().unwrap();
        for (_k, waiter) in m.iter() {
            waiter.cancel_wait();
        }
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
