# libsystemd

[![Build Status](https://travis-ci.org/lucab/libsystemd-rs.svg?branch=master)](https://travis-ci.org/lucab/libsystemd-rs)
[![crates.io](https://img.shields.io/crates/v/libsystemd.svg)](https://crates.io/crates/libsystemd)
[![LoC](https://tokei.rs/b1/github/lucab/libsystemd-rs?category=code)](https://github.com/lucab/libsystemd-rs)
[![Documentation](https://docs.rs/libsystemd/badge.svg)](https://docs.rs/libsystemd)

A pure-Rust client library to work with systemd.

It provides support to interact with systemd components available
on modern Linux systems. This crate is entirely implemented
in Rust, and does not require an external libsystemd dynamic library.

NB: this library is not yet features-complete. If you don't care about the C dependency, check [rust-systemd](https://github.com/jmesmon/rust-systemd).

## Example

```rust
use libsystemd::daemon::{self, NotifyState};

fn notify_ready() -> bool {
    if !daemon::booted() {
        println!("Not running systemd, early exit.");
        return false;
    };

    let sent = daemon::notify(true, &[NotifyState::Ready]).expect("notify failed");
    if !sent {
        println!("Notification not sent!");
    };
    sent
}
```

Some more examples are available under [examples](examples).

## License

Licensed under either of

 * Apache License, Version 2.0, [http://www.apache.org/licenses/LICENSE-2.0]
 * MIT license, [http://opensource.org/licenses/MIT]

at your option.

