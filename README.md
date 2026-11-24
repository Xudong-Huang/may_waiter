[![CI](https://github.com/Xudong-Huang/may_waiter/actions/workflows/ci.yml/badge.svg)](https://github.com/Xudong-Huang/may_waiter/actions/workflows/ci.yml)
[![Current Crates.io Version](https://img.shields.io/crates/v/may_waiter.svg)](https://crates.io/crates/may_waiter)
[![Document](https://img.shields.io/badge/doc-may_waiter-green.svg)](https://docs.rs/may_waiter)
# coroutine waiter library

This library provides blocking primitives for coroutines: one coroutine can wait
for a response produced by another coroutine or thread. `WaiterMap` associates
waiters with user supplied keys, `WaiterSlab` with slab ids, and `TokenWaiter`
needs no shared container at all. Waiting never holds a map/slab lock, and stale
ids are rejected instead of delivering a response to an unrelated waiter.


## Usage

* the map associated interface is through `WaiterMap`
```rust
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
```

* the token associated interface is through `TokenWaiter`, which does not need a caller-managed map
```rust
fn test_token_waiter() {
    for j in 0..100 {
        let result = go!(move || {
            let waiter = TokenWaiter::<usize>::new();
            let id = waiter.id().unwrap();
            // trigger the rsp in another coroutine
            go!(move || TokenWaiter::<usize>::set_rsp(id, j + 100).ok());
            // this will block until the rsp was set
            assert_eq!(waiter.wait_rsp(None).unwrap(), j + 100);
            // after wait we can get the id again
            let id = waiter.id().unwrap();
            go!(move || TokenWaiter::<usize>::set_rsp(id, j).ok());
            waiter.wait_rsp(std::time::Duration::from_secs(2)).unwrap()
        })
        .join()
        .unwrap();

        assert_eq!(result, j);
    }
}
```

`TokenWaiter` ids are plain `usize` values resolved through an internal
process-global registry. An id can be sent to another coroutine or thread, or
serialized (for example written to a network frame) and reconstructed with
`ID::from_usize`, as long as it is used in the process that issued it. Stale,
already consumed or wrongly typed ids are rejected, and `set_rsp` returns the
response back in `Err`. The waiter can be moved freely; it does not need to be
pinned.
