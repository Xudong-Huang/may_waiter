extern crate may_waiter;
#[macro_use]
extern crate may;

use may_waiter::{TokenWaiter, WaiterMap};

use std::sync::Arc;

fn test_waiter_map() {
    let req_map = Arc::new(WaiterMap::<usize, usize>::new());
    let req_map_1 = req_map.clone();

    let key = 1234;

    // one coroutine wait data send from another coroutine
    // prepare the waiter first
    let waiter = req_map.new_waiter(key);

    // trigger the rsp in another coroutine
    go!(move || req_map_1.set_rsp(&key, 100).ok());

    // this will block until the rsp was set
    let result = waiter.wait_rsp(None).unwrap();
    assert_eq!(result, 100);
}

fn test_token_waiter() {
    for j in 0..100 {
        let result = go!(move || {
            let waiter = TokenWaiter::<usize>::new();
            let id = waiter.id().unwrap();
            // trigger the rsp in another coroutine
            go!(move || TokenWaiter::set_rsp(id, j + 100));
            // this will block until the rsp was set
            assert_eq!(waiter.wait_rsp(None).unwrap(), j + 100);
            // after wait we can get the id again
            let id = waiter.id().unwrap();
            go!(move || TokenWaiter::set_rsp(id, j));
            waiter.wait_rsp(std::time::Duration::from_secs(2)).unwrap()
        })
        .join()
        .unwrap();

        assert_eq!(result, j);
    }
}

fn main() {
    test_waiter_map();
    test_token_waiter();
}
