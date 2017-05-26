use std::fmt::Debug;
use std::hash::Hash;
use std::time::Duration;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::io::{self, Error, ErrorKind};

use may::coroutine;
use may::sync::{AtomicOption, Mutex, Blocker};

struct Waiter<T> {
    blocker: Blocker,
    rsp: AtomicOption<T>,
}

impl<T> Waiter<T> {
    fn new() -> Self {
        Waiter {
            blocker: Blocker::new(false),
            rsp: AtomicOption::none(),
        }
    }

    fn set_rsp(&self, rsp: T) {
        // set the response
        self.rsp.swap(rsp, Ordering::Release);
        // wake up the blocker
        self.blocker.unpark();
    }
}

pub struct WaiterGuard<'a, K: Hash + Eq + 'a, T: 'a> {
    owner: &'a WaiterMap<K, T>,
    id: K,
}

impl<'a, K: Hash + Eq + Debug, T> WaiterGuard<'a, K, T> {
    // wait for response
    pub fn wait_rsp<D: Into<Option<Duration>>>(self, timeout: D) -> io::Result<T> {
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
    // TODO: use a special KEY to avoid the Hashmap
    map: Mutex<HashMap<K, Waiter<T>>>,
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
        m.insert(id.clone(), Waiter::new());
        WaiterGuard {
            owner: self,
            id: id,
        }
    }

    // used internally
    fn del_waiter(&self, id: &K) -> Option<Waiter<T>> {
        let mut m = self.map.lock().unwrap();
        m.remove(id)
    }

    fn wait_rsp(&self, id: &K, timeout: Option<Duration>) -> io::Result<T>
        where K: Debug
    {
        use self::coroutine::ParkError;

        let map = self.map.lock().unwrap();
        let waiter: &Waiter<T> = match map.get(&id) {
            Some(v) => unsafe { ::std::mem::transmute(v) },
            None => unreachable!("can't find id in waiter map!"),
        };
        //release the mutex
        drop(map);

        match waiter.blocker.park(timeout.into()) {
            Ok(_) => {
                match waiter.rsp.take(Ordering::Acquire) {
                    Some(rsp) => Ok(rsp),
                    None => panic!("unable to get the rsp, id={:?}", id),
                }
            }
            Err(ParkError::Timeout) => {
                // remove the req from req map
                error!("timeout zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz, id={:?}", id);
                Err(Error::new(ErrorKind::TimedOut, "wait rsp timeout"))
            }
            Err(ParkError::Canceled) => {
                error!("canceled id={:?}", id);
                coroutine::trigger_cancel_panic();
            }
        }
    }

    // set rsp for the corresponding waiter
    pub fn set_rsp(&self, id: &K, rsp: T) -> Result<(), T> {
        let m = self.map.lock().unwrap();
        match m.get(id) {
            Some(water) => {
                water.set_rsp(rsp);
                Ok(())
            }
            None => Err(rsp),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

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
