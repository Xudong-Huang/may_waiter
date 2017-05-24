#[macro_use]
extern crate log;
extern crate may;

use std::time::Duration;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::io::{Result, Error, ErrorKind};

use may::coroutine;
use may::sync::{AtomicOption, Mutex, Blocker};

pub struct WaitReq<T> {
    blocker: Blocker,
    rsp: AtomicOption<T>,
}

impl<T> WaitReq<T> {
    pub fn new() -> Self {
        WaitReq {
            blocker: Blocker::new(false),
            rsp: AtomicOption::none(),
        }
    }

    pub fn set_rsp(&self, rsp: T) {
        // set the response
        self.rsp.swap(rsp, Ordering::Release);
        // wake up the blocker
        self.blocker.unpark();
    }
}

pub struct WaitReqMap<T> {
    map: Mutex<HashMap<usize, *mut WaitReq<T>>>,
}

unsafe impl<T> Send for WaitReqMap<T> {}
unsafe impl<T> Sync for WaitReqMap<T> {}

impl<T> WaitReqMap<T> {
    pub fn new() -> Self {
        WaitReqMap { map: Mutex::new(HashMap::new()) }
    }

    pub fn add_req(&self, id: usize, req: &mut WaitReq<T>) {
        let mut m = self.map.lock().unwrap();
        m.insert(id, req as *mut _);
    }

    pub fn get_waiter(&self, id: usize) -> Option<&mut WaitReq<T>> {
        let mut m = self.map.lock().unwrap();
        m.remove(&id).map(|v| unsafe { &mut *v })
    }

    // wait for response
    pub fn wait_rsp<D: Into<Option<Duration>>>(&self,
                                               id: usize,
                                               timeout: D,
                                               req: &mut WaitReq<T>)
                                               -> Result<T> {
        use coroutine::ParkError;

        match req.blocker.park(timeout.into()) {
            Ok(_) => {
                match req.rsp.take(Ordering::Acquire) {
                    Some(frame) => Ok(frame),
                    None => panic!("unable to get the rsp, id={}", id),
                }
            }
            Err(ParkError::Timeout) => {
                // remove the req from req map
                error!("timeout zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz, id={}", id);
                self.get_waiter(id);
                Err(Error::new(ErrorKind::TimedOut, "wait rsp timeout"))
            }
            Err(ParkError::Canceled) => {
                error!("canceled id={}", id);
                self.get_waiter(id);
                coroutine::trigger_cancel_panic();
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        use std::sync::Arc;
        let req_map = Arc::new(WaitReqMap::<usize>::new());
        let rmap = req_map.clone();

        // one coroutine wait data send from another coroutine
        // prepare the waiter first
        let mut waiter = WaitReq::new();
        req_map.add_req(1234, &mut waiter);

        // trigger the rsp in another coroutine
        coroutine::spawn(move || {
                             // send out the response
                             rmap.get_waiter(1234).map(|req| req.set_rsp(100));
                         });

        // this will block until the rsp was set
        let result = req_map.wait_rsp(1234, None, &mut waiter).unwrap();
        assert_eq!(result, 100);

    }
}
