mod chain;
mod command;
mod issue_05;
#[cfg(feature = "send-mail")]
mod send_mail;


use std::collections::HashMap;
use std::str::FromStr;

use new_tokio_smtp::mock::{ActionData, Actor, MockSocket};
use new_tokio_smtp::{Capability, Connection, Domain, EhloData, EsmtpKeyword, Io};

pub fn mock(conv: Vec<(Actor, ActionData)>) -> Connection {
    let io: Io = MockSocket::new(conv).into();
    Connection::from(io)
}

pub fn mock_no_shutdown(conv: Vec<(Actor, ActionData)>) -> Connection {
    let io: Io = MockSocket::new_no_check_shutdown(conv).into();
    Connection::from(io)
}

pub fn with_capability(con: Connection, cap: &str) -> Connection {
    let capability = Capability::from(EsmtpKeyword::from_str(cap).unwrap());

    let (socket, buffer, opt_ehlo_data) = Io::from(con).split();

    let (domain, mut ehlo_map) = opt_ehlo_data
        .map(|ehlo_data| ehlo_data.into())
        .unwrap_or_else(|| (Domain::from_unchecked("uhmail.test"), HashMap::new()));

    ehlo_map.insert(capability, Vec::new());

    let ehlo_data = EhloData::from((domain, ehlo_map));

    Connection::from(Io::from((socket, buffer, ehlo_data)))
}
