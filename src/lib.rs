#[macro_use]
extern crate log;
extern crate may;

use std::fmt::Debug;
use std::hash::Hash;
use std::time::Duration;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::io::{self, Error, ErrorKind};

use may::coroutine;
use may::sync::{AtomicOption, Mutex, Blocker};

pub struct Waiter<K, T> {
    blocker: Blocker,
    rsp: AtomicOption<T>,
    id: K,
    wmap: *const WaiterMap<K, T>,
}

unsafe impl<K, T> Send for Waiter<K, T> {}
unsafe impl<K, T> Sync for Waiter<K, T> {}

impl<K: Hash + Eq, T> Waiter<K, T> {
    fn new(id: K, wmap: &WaiterMap<K, T>) -> Self {
        Waiter {
            id: id,
            blocker: Blocker::new(false),
            rsp: AtomicOption::none(),
            wmap: wmap,
        }
    }

    fn set_rsp(&self, rsp: T) {
        println!("p = {:p}", self);
        // set the response
        self.rsp.swap(rsp, Ordering::Release);
        // wake up the blocker
        self.blocker.unpark();
    }

    // wait for response
    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T>
        where K: Debug + Copy
    {
        use coroutine::ParkError;

        let id = self.id;
        let wmap = unsafe { &*self.wmap };
        match self.blocker.park(timeout.into()) {
            Ok(_) => {
                match self.rsp.take(Ordering::Acquire) {
                    Some(frame) => Ok(frame),
                    None => panic!("unable to get the rsp, id={:?}", id),
                }
            }
            Err(ParkError::Timeout) => {
                // remove the req from req map
                error!("timeout zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz, id={:?}", id);
                wmap.get_waiter(id);
                Err(Error::new(ErrorKind::TimedOut, "wait rsp timeout"))
            }
            Err(ParkError::Canceled) => {
                error!("canceled id={:?}", id);
                wmap.get_waiter(id);
                coroutine::trigger_cancel_panic();
            }
        }
    }
}

pub struct WaiterMap<K, T> {
    // TODO: use atomic hashmap instead
    // TODO: use a special KEY to avoid the Hashmap
    map: Mutex<HashMap<K, *const Waiter<K, T>>>,
}

unsafe impl<K, T> Send for WaiterMap<K, T> {}
unsafe impl<K, T> Sync for WaiterMap<K, T> {}

impl<K: Hash + Eq + Copy, T> WaiterMap<K, T> {
    pub fn new() -> Self {
        WaiterMap { map: Mutex::new(HashMap::new()) }
    }

    // return a waiter on the stack!
    // #[inline(always)]
    pub fn new_waiter(&self, id: K) -> Box<Waiter<K, T>> {
        let ret = Box::new(Waiter::new(id, self));
        self.add_waiter(ret.as_ref());
        ret
    }

    // the waiter must be alive on the stack!
    fn add_waiter(&self, req: &Waiter<K, T>) {
        let mut m = self.map.lock().unwrap();
        m.insert(req.id, req);
    }

    // used internally
    fn get_waiter(&self, id: K) -> Option<&Waiter<K, T>> {
        let mut m = self.map.lock().unwrap();
        m.remove(&id).map(|v| unsafe { &*v })
    }

    // set rsp for the corresponding waiter
    pub fn set_rsp(&self, id: K, rsp: T) -> Result<(), T> {
        match self.get_waiter(id) {
            Some(req) => {
                req.set_rsp(rsp);
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
    fn it_works() {
        use std::sync::Arc;
        let req_map = Arc::new(WaiterMap::<usize, usize>::new());
        let rmap = req_map.clone();

        // one coroutine wait data send from another coroutine
        // prepare the waiter first
        let waiter = req_map.new_waiter(1234);

        // trigger the rsp in another coroutine
        coroutine::spawn(move || {
                             // send out the response
                             rmap.set_rsp(1234, 100).ok();
                         });

        // this will block until the rsp was set
        let result = waiter.wait_rsp(None).unwrap();
        assert_eq!(result, 100);
    }
}
