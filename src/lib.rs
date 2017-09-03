//! A pure-Rust client library to work with systemd.
//!
//! It provides support to interact with systemd components available
//! on modern Linux systems. This crate is entirely implemented
//! in Rust, and does not require an external libsystemd dynamic library.
//!
//! ```rust
//! use libsystemd::daemon::{self, NotifyState};
//!
//! fn notify_ready() -> bool {
//!     if !daemon::booted() {
//!         println!("Not running systemd, early exit.");
//!         return false;
//!     };
//!
//!     let sent = daemon::notify(true, &[NotifyState::Ready]).expect("notify failed");
//!     if !sent {
//!         println!("Notification not sent!");
//!     };
//!     sent
//! }
//! ```

#[macro_use]
extern crate error_chain;
extern crate libc;
extern crate nix;
#[macro_use]
extern crate try_or;

/// Interfaces for systemd-aware daemons.
pub mod daemon;

/// Error handling.
pub mod errors;
