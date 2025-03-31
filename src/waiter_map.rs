use scc::HashMap;

use crate::waiter::Waiter;

use std::hash::Hash;
use std::io;
use std::sync::Arc;
use std::time::Duration;

pub struct MapWaiterOwned<K: Hash + Eq, T> {
    map: Arc<WaiterMap<K, T>>,
    id: K,
}

impl<K: Hash + Eq, T> MapWaiterOwned<K, T> {
    /// wait for response
    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T> {
        self.map.wait_rsp(&self.id, timeout.into())
    }

    /// set rsp for the waiter
    pub fn set_rsp(&self, rsp: T) -> Result<(), T> {
        self.map.set_rsp(&self.id, rsp)
    }

    /// get id
    pub fn id(&self) -> &K {
        &self.id
    }
}

impl<K: Hash + Eq, T> Drop for MapWaiterOwned<K, T> {
    fn drop(&mut self) {
        // remove the entry
        self.map.del_waiter(&self.id);
    }
}

/// Waiter guard to wait the response
#[derive(Debug)]
pub struct MapWaiter<'a, K: Hash + Eq + 'a, T: 'a> {
    owner: &'a WaiterMap<K, T>,
    id: K,
}

impl<K: Hash + Eq, T> MapWaiter<'_, K, T> {
    /// wait for response
    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T> {
        self.owner.wait_rsp(&self.id, timeout.into())
    }
}

impl<K: Hash + Eq, T> Drop for MapWaiter<'_, K, T> {
    fn drop(&mut self) {
        // remove the entry
        self.owner.del_waiter(&self.id);
    }
}

/// Waiter map that could be used to wait response for given keys
pub struct WaiterMap<K, T> {
    map: HashMap<K, Box<Waiter<T>>>,
}

impl<K: Hash + Eq, T> std::fmt::Debug for WaiterMap<K, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
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
            map: HashMap::new(),
        }
    }

    /// return a waiter on the stack!
    pub fn new_waiter(&self, id: K) -> MapWaiter<K, T>
    where
        K: Clone,
    {
        // if we add a same key, the old waiter would be lost!
        if self
            .map
            .insert(id.clone(), Box::new(Waiter::new()))
            .is_err()
        {
            panic!("key already exists in the map!")
        };
        MapWaiter { owner: self, id }
    }

    /// return an owned waiter
    /// don't pass the waiter from thread context to coroutine context
    /// or the waiter would block the coroutine runtime!
    pub fn new_waiter_owned(self: &Arc<Self>, id: K) -> MapWaiterOwned<K, T>
    where
        K: Clone,
    {
        // if we add a same key, the old waiter would be lost!
        if self
            .map
            .insert(id.clone(), Box::new(Waiter::new()))
            .is_err()
        {
            panic!("key already exists in the map!")
        };
        MapWaiterOwned {
            map: self.clone(),
            id,
        }
    }

    // used internally
    fn del_waiter(&self, id: &K) -> Option<(K, Box<Waiter<T>>)> {
        self.map.remove(id)
    }

    fn wait_rsp(&self, id: &K, timeout: Option<Duration>) -> io::Result<T> {
        fn extend_lifetime<'a, T>(r: &T) -> &'a T {
            unsafe { ::std::mem::transmute(r) }
        }

        let waiter = match self.map.get(id) {
            // extends the lifetime of the waiter ref
            Some(v) => extend_lifetime(v.as_ref()),
            None => unreachable!("can't find id in waiter map!"),
        };

        waiter.wait_rsp(timeout)
    }

    /// set rsp for the corresponding waiter
    pub fn set_rsp(&self, id: &K, rsp: T) -> Result<(), T> {
        match self.map.get(id) {
            Some(waiter) => {
                waiter.set_rsp(rsp);
                Ok(())
            }
            None => Err(rsp),
        }
    }

    /// cancel all the waiting waiter, all wait would return NotFound error
    pub fn cancel_all(&self) {
        self.map.scan(|_k, waiter| {
            waiter.cancel_wait();
        });
    }

    /// for each waiter in the map execute the function
    /// this is used to notify all waiters
    pub fn for_each(&self, f: impl Fn(&K, &Box<Waiter<T>>)) {
        self.map.scan(|k, v| {
            f(k, v);
        });
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

    #[test]
    fn test_map_waiter() {
        use std::sync::Arc;
        let req_map = Arc::new(WaiterMap::<usize, usize>::new());
        let key = 1234;

        // one coroutine wait data send from another coroutine
        // prepare the waiter first
        let waiter = Arc::new(req_map.new_waiter_owned(key));
        let waiter_1 = waiter.clone();

        // trigger the rsp in another coroutine
        go!(move || { waiter_1.set_rsp(100).ok() });

        // this will block until the rsp was set
        let result = waiter.wait_rsp(None).unwrap();
        assert_eq!(result, 100);
    }

    #[test]
    fn test_cancel_all() {
        use std::sync::Arc;
        let req_map = Arc::new(WaiterMap::<usize, usize>::new());
        let req_map_1 = req_map.clone();
        let key = 1234;

        let j = go!(move || {
            let waiter = req_map_1.new_waiter(key);
            let _ = waiter.wait_rsp(None);
        });

        std::thread::sleep(std::time::Duration::from_millis(100));
        req_map.cancel_all();
        unsafe { j.coroutine().cancel() };
        assert!(j.join().is_err());
    }
}
