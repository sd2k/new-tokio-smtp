use std::io as std_io;

use futures::{future, Future};

use new_tokio_smtp::chain::{HandleErrorInChain, OnError};
use new_tokio_smtp::error::LogicError;
use new_tokio_smtp::mock::{ActionData, Actor};
use new_tokio_smtp::{command, Connection};

use self::ActionData::*;
use self::Actor::*;

use super::mock;

#[test]
fn runs_the_cmd_chain() {
    let con = mock(vec![
        (Client, Lines(vec!["VRFY test1"])),
        (Server, Lines(vec!["250 1itus <testitus1@test.test>"])),
        (Client, Lines(vec!["VRFY test2"])),
        (Server, Lines(vec!["250 2itus <testitus2@test.test>"])),
        (Client, Lines(vec!["VRFY test3"])),
        (Server, Lines(vec!["250 3itus <testitus3@test.test>"])),
    ]);
    let chain = smtp_chain!(con with OnError::StopAndReset => [
        command::Verify { query: "test1".to_owned() },
        command::Verify { query: "test2".to_owned() },
        command::Verify { query: "test3".to_owned() }
    ])
    .and_then(|(con, res)| {
        assert!(res.is_ok());
        con.shutdown()
    });

    chain.wait().unwrap();
}

#[test]
fn stops_on_error() {
    let con = mock(vec![
        (Client, Lines(vec!["VRFY test1"])),
        (Server, Lines(vec!["250 1itus <testitus1@test.test>"])),
        (Client, Lines(vec!["VRFY test2"])),
        (Server, Lines(vec!["550 only 1itus was left behind"])),
    ]);
    let chain = smtp_chain!(con with OnError::Stop => [
        command::Verify { query: "test1".to_owned() },
        command::Verify { query: "test2".to_owned() },
        command::Verify { query: "test3".to_owned() }
    ])
    .and_then(|(con, res)| {
        assert!(res.is_err());
        con.shutdown()
    });

    chain.wait().unwrap();
}

#[test]
fn sends_reset_on_error_if_requested() {
    let con = mock(vec![
        (Client, Lines(vec!["VRFY test1"])),
        (Server, Lines(vec!["250 1itus <testitus1@test.test>"])),
        (Client, Lines(vec!["VRFY test2"])),
        (Server, Lines(vec!["550 only 1itus was left behind"])),
        (Client, Lines(vec!["RSET"])),
        (Server, Lines(vec!["250 Ok"])),
    ]);
    let chain = smtp_chain!(con with OnError::StopAndReset => [
        command::Verify { query: "test1".to_owned() },
        command::Verify { query: "test2".to_owned() },
        command::Verify { query: "test3".to_owned() }
    ])
    .and_then(|(con, res)| {
        assert!(res.is_err());
        con.shutdown()
    });

    chain.wait().unwrap();
}

struct IgnoreAllErrors;

impl HandleErrorInChain for IgnoreAllErrors {
    type Fut = future::FutureResult<(Connection, bool), std_io::Error>;

    fn handle_error(&self, con: Connection, _msg_idx: usize, _error: &LogicError) -> Self::Fut {
        future::ok((con, false))
    }
}

#[test]
fn ignores_error_if_requested() {
    let con = mock(vec![
        (Client, Lines(vec!["VRFY test1"])),
        (Server, Lines(vec!["250 1itus <testitus1@test.test>"])),
        (Client, Lines(vec!["VRFY test2"])),
        (Server, Lines(vec!["550 foobarror"])),
        (Client, Lines(vec!["VRFY test3"])),
        (Server, Lines(vec!["250 3itus <testitus3@test.test>"])),
    ]);
    let chain = smtp_chain!(con with IgnoreAllErrors => [
        command::Verify { query: "test1".to_owned() },
        command::Verify { query: "test2".to_owned() },
        command::Verify { query: "test3".to_owned() }
    ])
    .and_then(|(con, res)| {
        assert!(res.is_ok());
        con.shutdown()
    });

    chain.wait().unwrap();
}
