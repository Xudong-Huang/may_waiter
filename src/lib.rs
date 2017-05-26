#[macro_use]
extern crate log;
extern crate may;
extern crate rand;
extern crate base64;
extern crate crypto;

mod waiter_map;
mod waiter_token;

pub use waiter_map::{WaiterMap, WaiterGuard};
pub use waiter_token::{WaiterToken, Waiter};