//FIXME see if we can put this into Cargo.toml
#[cfg(not(feature="mock_impl"))]
compile_error!("integration tests require \"mock_impl\" feature");

#[macro_use]
extern crate new_tokio_smtp;
extern crate futures;

use new_tokio_smtp::mock::{MockSocket, Actor, ActionData};
use new_tokio_smtp::{Connection, Io};

mod command;
mod chain;

fn mock(conv: Vec<(Actor, ActionData)>) -> Connection {
    let io: Io = MockSocket::new(conv).into();
    Connection::from(io)
}

fn mock_no_shutdown(conv: Vec<(Actor, ActionData)>) -> Connection {
    let io: Io = MockSocket::new_no_check_shutdown(conv).into();
    Connection::from(io)
}