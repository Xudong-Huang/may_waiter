# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Note: versions 0.1.4, 0.1.5 and 0.1.8 have no tags in this repository, so their
contents are not listed here.

## [0.2.0] - 2026-11-24

### Changed

- **Breaking:** `TokenWaiter` ids are now process-global registry keys rather
  than waiter addresses. `ID` remains a `usize` newtype with
  `From<ID> for usize`, but `ID::from_usize` is now safe: using an id after the
  waiter is dropped, or an id issued for a different response type, is rejected
  instead of dereferencing freed memory. The waiter no longer needs to be
  pinned, but `TokenWaiter<T>` now requires `T: Send + 'static`.
- **Breaking:** `TokenWaiter::set_rsp` returns `Result<(), T>` and hands the
  response back when the id is stale or consumed, or when the waiter is gone.
  It previously returned `()` and silently discarded such responses.
- `WaiterMap` is backed by `dashmap` instead of `scc`.
- `WaiterMap` stores waiters as `Arc<Waiter<T>>` and clones one out before
  blocking, so `wait_rsp` no longer holds the map's shard lock for the duration
  of the wait. This replaces the previous unsafe lifetime transmute with a safe
  `Arc` clone.
- `Waiter` stores the response in `AtomicOption<T>` directly instead of
  `AtomicOption<Box<T>>`, avoiding a heap allocation per response.
- `WaiterMap::new_waiter`/`new_waiter_owned` insert through the entry API: a
  duplicate key still panics, but the existing waiter is left untouched and no
  unowned entry is leaked.

### Added

- Re-exported `token_waiter::Error` so the error returned by `TokenWaiter::id`
  can be named.
- Crate-level and public API documentation, including the locking and stale-id
  guarantees of each container.
- Tests for serialized-id round trips, stale and wrongly typed token ids,
  moving a `TokenWaiter`, timeouts, and duplicate `WaiterMap` keys.
- GitHub Actions CI: tests on Linux, macOS and Windows with stable Rust, plus
  rustfmt, clippy (`-D warnings`) and rustdoc (`-D warnings`) checks.

### Removed

- Travis CI configuration.

## [0.1.17] - 2025-04-01

### Changed

- Updated `scc` to 2.3.
- `WaiterMap` stays on `scc`; an experimental `dashmap` switch was reverted.

### Added

- Cancellation test for `WaiterMap`.

### Fixed

- Clippy warnings.

## [0.1.16] - 2024-07-12

### Added

- `WaiterSlab`, a slab based waiter container.

### Changed

- `WaiterMap` now uses `scc::HashMap`.
- Renamed the borrowed guard to `MapWaiter` and the owned one to
  `MapWaiterOwned`; `make_waiter` became `new_waiter_owned`.

## [0.1.15] - 2024-07-10

### Changed

- `ID::from_usize` takes a `usize` and validates that it is non-zero.
- Removed the unnecessary `Debug` bound on the waiter guard.

## [0.1.14] - 2024-07-10

### Added

- `MapWaiter::set_rsp`.

## [0.1.13] - 2024-07-06

### Changed

- `MapWaiter` is now public.

## [0.1.12] - 2024-07-06

### Added

- `MapWaiter`, an owned waiter handle obtained from `make_waiter`.

### Changed

- `WaiterMap` uses a `BTreeMap` instead of a `HashMap`, so its key type is
  `Ord` rather than `Hash + Eq`.

## [0.1.11] - 2024-07-06

### Changed

- `TokenWaiter` ids use `NonZeroUsize` internally instead of a raw `usize`.

## [0.1.10] - 2024-03-06

### Changed

- `Waiter` is now public.

## [0.1.9] - 2023-01-30

### Changed

- Updated `may` to 0.3.32.
- `Waiter::set_rsp` uses `AtomicOption::store` instead of `swap`.

## [0.1.7] - 2022-09-19

### Changed

- Reverted `WaiterMap` from `dashmap` back to a mutex protected `HashMap`,
  because the dashmap based implementation was buggy.
- Removed unnecessary code.

## [0.1.6] - 2022-04-08

### Added

- `TokenWaiter`, a map-free token based waiter.
- `ID` type for `TokenWaiter`, including an unsafe `from_usize` constructor.

### Changed

- Refined the token waiter API.

## [0.1.3] - 2022-03-28

### Changed

- `WaiterMap` switched from a mutex protected `HashMap` to `dashmap`.

## [0.1.2] - 2019-04-15

### Changed

- `Waiter` is now public.

## [0.1.1] - 2019-01-31

### Added

- Initial release: map based waiter guards for waiting on a response produced
  by another coroutine.
- `cancel_all` API.
- README, LICENSE and Travis CI.

### Changed

- Updated the `may` dependency to 0.3.

[0.2.0]: https://github.com/Xudong-Huang/may_waiter/compare/0.1.17...0.2.0
[0.1.17]: https://github.com/Xudong-Huang/may_waiter/compare/0.1.16...0.1.17
[0.1.16]: https://github.com/Xudong-Huang/may_waiter/compare/0.1.15...0.1.16
[0.1.15]: https://github.com/Xudong-Huang/may_waiter/compare/0.1.14...0.1.15
[0.1.14]: https://github.com/Xudong-Huang/may_waiter/compare/0.1.13...0.1.14
[0.1.13]: https://github.com/Xudong-Huang/may_waiter/compare/0.1.12...0.1.13
[0.1.12]: https://github.com/Xudong-Huang/may_waiter/compare/0.1.11...0.1.12
[0.1.11]: https://github.com/Xudong-Huang/may_waiter/compare/0.1.10...0.1.11
[0.1.10]: https://github.com/Xudong-Huang/may_waiter/compare/0.1.9...0.1.10
[0.1.9]: https://github.com/Xudong-Huang/may_waiter/compare/0.1.7...0.1.9
[0.1.7]: https://github.com/Xudong-Huang/may_waiter/compare/0.1.6...0.1.7
[0.1.6]: https://github.com/Xudong-Huang/may_waiter/compare/v0.1.3...0.1.6
[0.1.3]: https://github.com/Xudong-Huang/may_waiter/compare/0.1.2...v0.1.3
[0.1.2]: https://github.com/Xudong-Huang/may_waiter/compare/0.1.1...0.1.2
[0.1.1]: https://github.com/Xudong-Huang/may_waiter/releases/tag/0.1.1
