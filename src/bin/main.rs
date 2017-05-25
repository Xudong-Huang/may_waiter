extern crate may;
extern crate waiter_map;

use may::coroutine;
use waiter_map::WaiterMap;

fn main() {
    use std::sync::Arc;
    let req_map = Arc::new(WaiterMap::<usize, usize>::new());
    let rmap = req_map.clone();

    // one coroutine wait data send from another coroutine
    // prepare the waiter first
    let waiter = req_map.new_waiter(1234);

    // trigger the rsp in another coroutine
    coroutine::spawn(move || {
                         // send out the response
                         rmap.set_rsp(&1234, 100).ok();
                     });

    // this will block until the rsp was set
    let result = waiter.wait_rsp(None).unwrap();
    assert_eq!(result, 100);
}