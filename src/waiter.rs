use may::coroutine;
use may::sync::{AtomicOption, Blocker};

use std::time::Duration;
use std::{fmt, io};

/// Generic Waiter that could wait for a response
pub struct Waiter<T> {
    blocker: Blocker,
    rsp: AtomicOption<Box<T>>,
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
        self.rsp.store(Box::new(rsp));
        // wake up the blocker
        self.blocker.unpark();
    }

    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T> {
        use may::coroutine::ParkError;
        use std::io::{Error, ErrorKind};
        let timeout = timeout.into();
        match self.blocker.park(timeout) {
            Ok(_) => match self.rsp.take() {
                Some(rsp) => Ok(*rsp),
                None => unreachable!("unable to get the rsp"),
            },
            Err(ParkError::Timeout) => Err(Error::new(ErrorKind::TimedOut, "wait rsp timeout")),
            Err(ParkError::Canceled) => {
                coroutine::trigger_cancel_panic();
            }
        }
    }

    pub fn cancel_wait(&self) {
        // wake up the blocker without rsp
        self.blocker.unpark()
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
