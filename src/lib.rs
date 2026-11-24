//! Coroutine-aware waiters: blocking primitives that let one coroutine wait
//! for a response produced by another coroutine or thread.
//!
//! The crate provides three interfaces:
//!
//! * [`WaiterMap`] associates a waiter with a user supplied key. Every
//!   operation hashes the key and locates the map entry.
//! * [`WaiterSlab`] associates a waiter with a slab id, avoiding hashing. Slab
//!   ids carry a generation check, so a stale id never resolves to a recycled
//!   slot.
//! * [`TokenWaiter`] needs no caller-managed map or slab: the waiter stays
//!   with its owner while its [`ID`] is sent to the responder. The id is a
//!   plain `usize` resolved through an internal process-global registry, so it
//!   can be serialized (e.g. written to a network frame) and reconstructed with
//!   [`ID::from_usize`], as long as it is used in the process that issued it.
//!
//! Waiting on a map or slab entry does not hold a lock: the waiter is cloned or
//! ref-counted out before blocking. Stale ids are rejected with the response
//! returned to the caller, so a response is never delivered to an unrelated
//! waiter.
//!
//! # Example
//!
//! ```no_run
//! use may::go;
//! use may_waiter::{TokenWaiter, ID};
//!
//! let waiter = TokenWaiter::<usize>::new();
//! let id: usize = waiter.id().unwrap().into();
//!
//! // send the id to the coroutine that produces the response; it can also
//! // travel as plain data, e.g. in a network frame
//! go!(move || {
//!     let id = ID::from_usize(id);
//!     TokenWaiter::<usize>::set_rsp(id, 42).unwrap();
//! });
//!
//! // block the current coroutine until the response arrives
//! assert_eq!(waiter.wait_rsp(None).unwrap(), 42);
//! ```

mod token_waiter;
mod waiter;
mod waiter_map;
mod waiter_slab;

pub use token_waiter::{Error, TokenWaiter, ID};
pub use waiter::Waiter;
pub use waiter_map::{MapWaiter, MapWaiterOwned, WaiterMap};
pub use waiter_slab::{SlabWaiter, SlabWaiterOwned, WaiterSlab};
