use std::io;
use std::fmt::Debug;
use std::hash::Hash;
use std::time::Duration;
use std::collections::HashMap;

use Waiter;
use may::sync::Mutex;

pub struct WaiterGuard<'a, K: Hash + Eq + 'a, T: 'a> {
    owner: &'a WaiterMap<K, T>,
    id: K,
}

impl<'a, K: Hash + Eq + Debug, T> WaiterGuard<'a, K, T> {
    // wait for response
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

pub struct WaiterMap<K, T> {
    // TODO: use atomic hashmap instead
    map: Mutex<HashMap<K, Box<Waiter<T>>>>,
}


unsafe impl<K, T> Send for WaiterMap<K, T> {}
unsafe impl<K, T> Sync for WaiterMap<K, T> {}

impl<K: Hash + Eq, T> WaiterMap<K, T> {
    pub fn new() -> Self {
        WaiterMap { map: Mutex::new(HashMap::new()) }
    }

    // return a waiter on the stack!
    pub fn new_waiter<'a>(&'a self, id: K) -> WaiterGuard<'a, K, T>
        where K: Clone
    {
        let mut m = self.map.lock().unwrap();
        // if we add a same key, the old waiter would be lost!
        m.insert(id.clone(), Box::new(Waiter::new()));
        WaiterGuard {
            owner: self,
            id: id,
        }
    }

    // used internally
    fn del_waiter(&self, id: &K) -> Option<Box<Waiter<T>>> {
        let mut m = self.map.lock().unwrap();
        m.remove(id)
    }

    fn wait_rsp(&self, id: &K, timeout: Option<Duration>) -> io::Result<T>
        where K: Debug
    {
        fn extend_lifetime<'a, T>(r: &T) -> &'a T {
            unsafe { ::std::mem::transmute(r) }
        }

        let map = self.map.lock().unwrap();
        let waiter = match map.get(&id) {
            // extends the lifetime of the waiter ref
            Some(v) => extend_lifetime(v.as_ref()),
            None => unreachable!("can't find id in waiter map!"),
        };

        //release the mutex
        drop(map);

        waiter.wait_rsp(timeout)
    }

    // set rsp for the corresponding waiter
    pub fn set_rsp(&self, id: &K, rsp: T) -> Result<(), T>
        where K: Debug
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
}


#[cfg(test)]
mod tests {
    use super::*;
    use may::coroutine;

    #[test]
    fn test_waiter_map() {
        use std::sync::Arc;
        let req_map = Arc::new(WaiterMap::<usize, usize>::new());
        let rmap = req_map.clone();

        let key = 1234;

        // one coroutine wait data send from another coroutine
        // prepare the waiter first
        let waiter = req_map.new_waiter(key);

        // trigger the rsp in another coroutine
        coroutine::spawn(move || rmap.set_rsp(&key, 100).ok());

        // this will block until the rsp was set
        let result = waiter.wait_rsp(None).unwrap();
        assert_eq!(result, 100);
    }
}
