extern crate may;
extern crate co_waiter;

use std::sync::Arc;
use may::coroutine;
use co_waiter::WaiterMap;
use co_waiter::{WaiterToken, Waiter};

fn test_waiter_map() {
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

fn test_waiter_token() {
    let req_map = Arc::new(WaiterToken::new());
    let rmap = req_map.clone();

    // one coroutine wait data send from another coroutine
    // prepare the waiter first
    let waiter = Waiter::<usize>::new();
    let token = req_map.waiter_to_token(&waiter);
    println!("token={}", token);
    // trigger the rsp in another coroutine
    coroutine::spawn(move || rmap.token_to_waiter(&token).map(|w| w.set_rsp(100)));

    // this will block until the rsp was set
    let result = waiter.wait_rsp(None).unwrap();
    assert_eq!(result, 100);
}

fn main() {
    test_waiter_map();
    test_waiter_token();
}