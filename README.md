[![Build Status](https://travis-ci.org/Xudong-Huang/may_waiter.svg?branch=master)](https://travis-ci.org/Xudong-Huang/may_waiter)
[![Current Crates.io Version](https://img.shields.io/crates/v/may_waiter.svg)](https://crates.io/crates/may_waiter)
[![Document](https://img.shields.io/badge/doc-may_waiter-green.svg)](https://docs.rs/may_waiter)
# coroutine waiter library

This library provide a map associated blocking primitive that waiting for a response produced by another coroutine


## Usage

* the map associated interface is through `WaiterMap`
```rust
fn test_waiter_map() {
    let req_map = Arc::new(WaiterMap::<usize, usize>::new());
    let rmap = req_map.clone();

    let key = 1234;

    // one coroutine wait data send from another coroutine
    // prepare the waiter first
    let waiter = req_map.new_waiter(key);

    // trigger the rsp in another coroutine
    go!(move || rmap.set_rsp(&key, 100).ok());

    // this will block until the rsp was set
    let result = waiter.wait_rsp(None).unwrap();
    assert_eq!(result, 100);
}
```
