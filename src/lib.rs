//! The new-tokio-smtp crate provides an extendible SMTP (Simple Mail Transfer Protocol)
//! implementation using tokio.
//!
//! This crate provides _only_ SMTP functionality, this means it does neither
//! provides functionality for creating mails, nor for e.g. retrying sending
//! a mail if the receiver was temporary not available.
//!
//! This crate can be seen from two perspectives:
//!
//! 1. a normal API user, mainly bothering with `ConnectionConfig`, `Connection`
//!    and `Cmd` implementations (in the `command` module)
//!
//! 2. a cmd implementation, having to use `Io`, `Socket` etc.
//!
//! # Features
//!
//! ## `send_mail`
//!
//! While still not handling the creation/encoding of mails if this feature is
//! enabled a `send_mail` command is added `Connection` which combines the steps
//! of sending the `MAIL` command, the `RCPT` command and the `DATA` command.
//!
//! ## `mock-support`, `mock-impl`
//!
//! Extend the `Socket` abstraction to include a mock socket additional to `Tcp`, `TcpTls`.
//! Also provides a mock socket implementation for simply testing commands. Custom implementations
//! can be provided too if needed for testing
//!

// I use `{ ...; let fut = ...long multi line; fut }` a lot for better readability.
// it also makes it so much easier to wrap the return value into a `dbg!`, `Box::new` and similar.
#![allow(clippy::let_and_return)]

#[macro_use]
extern crate futures;
extern crate base64;
extern crate bytes;
extern crate hostname;
extern crate native_tls;
#[cfg(feature = "mock-impl")]
extern crate rand;
extern crate tokio;
extern crate tokio_tls;
#[cfg(feature = "send-mail")]
extern crate vec1;
// order of modules is also "order" in dependency-tree
// i.e. module should only import from modules hither
// up in the list
mod ascii;
mod data_types;
pub mod future_ext;
#[macro_use]
mod common;
pub mod chain;
pub mod command;
mod connect;
mod connection;
pub mod error;
pub mod io;
#[cfg(feature = "mock-impl")]
pub mod mock;
pub mod response;
#[cfg(feature = "send-mail")]
pub mod send_mail;

pub use self::common::*;
pub use self::connect::*;
pub use self::connection::*;
pub use self::data_types::*;
pub use self::io::Io;
pub use self::response::Response;
