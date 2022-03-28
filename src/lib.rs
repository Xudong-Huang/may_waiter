#[cfg_attr(test, macro_use)]
extern crate may;

mod waiter;
mod waiter_map;

pub use waiter_map::{WaiterGuard, WaiterMap};
