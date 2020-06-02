//Note: If we add more thinks to this file use a macro which
//      adds a cfg(feature="mock-impl") to all items.
#[cfg(not(feature = "mock-impl"))]
compile_error!("integration tests require \"mock-impl\" feature");


#[cfg(feature = "mock-impl")]
mod chain;
#[cfg(feature = "mock-impl")]
mod command;
#[cfg(feature = "mock-impl")]
mod issue_05;
#[cfg(all(feature = "send-mail", feature = "mock-impl"))]
mod send_mail;

#[cfg(feature = "mock-impl")]
pub use self::_main::*;

#[cfg(feature = "mock-impl")]
mod _main {
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
}
