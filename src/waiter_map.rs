use dashmap::DashMap as HashMap;
use dashmap::Entry;

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
    map: HashMap<K, Arc<Waiter<T>>>,
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
    pub fn new_waiter(&self, id: K) -> MapWaiter<'_, K, T>
    where
        K: Clone,
    {
        self.insert_waiter(id.clone());
        MapWaiter { owner: self, id }
    }

    /// return an owned waiter
    /// don't pass the waiter from thread context to coroutine context
    /// or the waiter would block the coroutine runtime!
    pub fn new_waiter_owned(self: &Arc<Self>, id: K) -> MapWaiterOwned<K, T>
    where
        K: Clone,
    {
        self.insert_waiter(id.clone());
        MapWaiterOwned {
            map: self.clone(),
            id,
        }
    }

    // insert a new waiter for the key, panic if the key already exists
    fn insert_waiter(&self, id: K) {
        // `entry` acquires the shard's write lock and holds it inside the
        // returned `Entry`, so the occupancy check and the insert are atomic:
        // threads inserting the same key serialize on that lock. Since
        // `K: Hash + Eq`, equal keys always map to the same shard, so the
        // loser of the race observes `Occupied` and panics instead of
        // overwriting the winner's waiter.
        match self.map.entry(id) {
            // panic without touching the existing entry, so that a waiter
            // already waiting on this key would not be lost
            Entry::Occupied(_) => panic!("key already exists in the map!"),
            Entry::Vacant(entry) => {
                // the returned `RefMut` is dropped right away, releasing the
                // shard lock before this function returns
                entry.insert(Arc::new(Waiter::new()));
            }
        }
    }

    // used internally
    fn del_waiter(&self, id: &K) -> Option<(K, Arc<Waiter<T>>)> {
        self.map.remove(id)
    }

    fn wait_rsp(&self, id: &K, timeout: Option<Duration>) -> io::Result<T> {
        // clone the waiter out so that the shard read lock is released before
        // blocking below. Holding the `Ref` across `wait_rsp` would keep the
        // lock for the whole wait, and since dashmap's shard lock is
        // writer-biased, a pending writer (e.g. another `new_waiter`/`drop` on
        // the same shard) would block a concurrent `set_rsp` forever.
        let waiter = match self.map.get(id) {
            Some(waiter) => Arc::clone(waiter.value()),
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
        self.map.iter().for_each(|waiter| {
            waiter.cancel_wait();
        });
    }

    /// for each waiter in the map execute the function
    /// this is used to notify all waiters
    pub fn for_each(&self, f: impl Fn(&K, &Arc<Waiter<T>>)) {
        self.map.iter().for_each(|waiter| {
            f(waiter.key(), waiter.value());
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
    fn test_duplicate_key() {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        use std::sync::Arc;

        let req_map = Arc::new(WaiterMap::<usize, usize>::new());
        let waiter = req_map.new_waiter(1234);

        // inserting a duplicate key panics, but the existing entry is untouched
        let result = catch_unwind(AssertUnwindSafe(|| {
            req_map.new_waiter(1234);
        }));
        assert!(result.is_err());

        // the original waiter is still in the map and can still get its rsp
        req_map.set_rsp(&1234, 100).unwrap();
        assert_eq!(waiter.wait_rsp(None).unwrap(), 100);
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
