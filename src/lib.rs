#[macro_use]
extern crate log;
extern crate may;
extern crate rand;
extern crate base64;
extern crate crypto;

use std::{io, fmt};
use std::time::Duration;
use std::sync::atomic::Ordering;

use may::coroutine;
use may::sync::{AtomicOption, Blocker};

mod waiter_map;
mod waiter_token;

pub struct Waiter<T> {
    blocker: Blocker,
    rsp: AtomicOption<T>,
}

impl<T> Waiter<T> {
    pub fn new() -> Self {
        Waiter {
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

    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T> {
        use io::{Error, ErrorKind};
        use coroutine::ParkError;

        match self.blocker.park(timeout.into()) {
            Ok(_) => {
                match self.rsp.take(Ordering::Acquire) {
                    Some(rsp) => Ok(rsp),
                    None => panic!("unable to get the rsp, waiter={:p}", &self),
                }
            }
            Err(ParkError::Timeout) => {
                error!("waiter timeout {:p}", &self);
                Err(Error::new(ErrorKind::TimedOut, "wait rsp timeout"))
            }
            Err(ParkError::Canceled) => {
                error!("waiter canceled {:p}", &self);
                coroutine::trigger_cancel_panic();
            }
        }
    }
}

impl<T> fmt::Debug for Waiter<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Waiter{{ ... }}")
    }
}

impl<T> Default for Waiter<T> {
    fn default() -> Self {
        Waiter::new()
    }
}

pub use waiter_token::WaiterToken;
pub use waiter_map::{WaiterMap, WaiterGuard};