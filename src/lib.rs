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
//! ## `mock_support`, `mock_impl`
//!
//! Extend the `Socket` abstraction to include a mock socket additional to `Tcp`, `TcpTls`.
//! Also provides a mock socket implementation for simply testing commands. Custom implementations
//! can be provided too if needed for testing

#[macro_use]
extern crate futures;
extern crate bytes;
extern crate tokio;
extern crate tokio_tls;
extern crate native_tls;
extern crate base64;
#[cfg(feature="mock_impl")]
extern crate rand;
#[cfg(feature="send_mail")]
extern crate vec1;
// order of modules is also "order" in dependency-tree
// i.e. module should only import from modules hither
// up in the list
pub mod future_ext;
mod ascii;
mod data_types;
#[macro_use]
mod common;
pub mod response;
pub mod error;
pub mod io;
mod connection;
mod connect;
pub mod command;
pub mod chain;
#[cfg(feature="mock_impl")]
pub mod mock;
#[cfg(feature="send_mail")]
pub mod send_mail;

pub use self::data_types::*;
pub use self::common::*;
pub use self::response::Response;
pub use self::io::Io;
pub use self::connection::*;
pub use self::connect::*;


