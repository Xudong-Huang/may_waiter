use may::coroutine;
use may::sync::{AtomicOption, Blocker};

use std::time::Duration;
use std::{fmt, io};

/// Generic waiter that blocks the current coroutine (or thread) until a
/// response is set.
///
/// A waiter stores a single response; it should not be waited on by two
/// places at the same time, since only one of them would receive it.
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

    /// Store the response and wake up the blocked waiter.
    pub fn set_rsp(&self, rsp: T) {
        // set the response
        self.rsp.store(rsp);
        // wake up the blocker
        self.blocker.unpark();
    }

    /// Block until a response is set.
    ///
    /// Returns an error with kind [`io::ErrorKind::TimedOut`] if the timeout
    /// expires. If the coroutine is cancelled while waiting, the cancellation
    /// panic is triggered.
    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T> {
        use may::coroutine::ParkError;
        use std::io::{Error, ErrorKind};
        let timeout = timeout.into();
        match self.blocker.park(timeout) {
            Ok(_) => match self.rsp.take() {
                Some(rsp) => Ok(rsp),
                None => unreachable!("unable to get the rsp"),
            },
            Err(ParkError::Timeout) => Err(Error::new(ErrorKind::TimedOut, "wait rsp timeout")),
            Err(ParkError::Canceled) => {
                coroutine::trigger_cancel_panic();
            }
        }
    }

    /// Wake up the blocked waiter without storing a response.
    ///
    /// Note: `wait_rsp` panics if it wakes up without a response, so cancelling
    /// waiters this way leaves their coroutines hitting that panic.
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
