# coroutine waiter library

this library provide a map associated or token associated bloking
primitive that waiting for a response.

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
    coroutine::spawn(move || rmap.set_rsp(&key, 100).ok());

    // this will block until the rsp was set
    let result = waiter.wait_rsp(None).unwrap();
    assert_eq!(result, 100);
}
```

* the token associated interface is through `WaiterToken and Waiter`
```rust
fn test_waiter_token() {
    use std::time::Duration;
    let req_toker = Arc::new(WaiterToken::new());
    let rtoker = req_toker.clone();

    // one coroutine wait data send from another coroutine
    // prepare the waiter first
    let waiter = Waiter::<usize>::new();
    let token = req_toker.waiter_to_token(&waiter);
    println!("token={}", token);
    // trigger the rsp in another coroutine
    coroutine::spawn(move || rtoker.token_to_waiter(&token).map(|w| w.set_rsp(100)));

    // this will block until the rsp was set
    let result = waiter.wait_rsp(Duration::from_millis(100)).unwrap();
    assert_eq!(result, 100);
}
```

> **Note**
> the `WaiterToken` interface doesn't need to maintain a map inside.
> instead we use a safe token to calculate the correct `Waiter` 
>
