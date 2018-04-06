extern crate new_tokio_smtp;
extern crate futures;

use futures::Future;

use new_tokio_smtp::{
    command, Connection,
    ClientIdentity
};
use new_tokio_smtp::io::{
    Io, Socket
};
use new_tokio_smtp::mock::{
    ActionData, Actor, MockSocket
};

use self::Actor::*;
use self::ActionData::*;

//FIXME see if we can put this into Cargo.toml
#[cfg(not(feature="mock_impl"))]
compile_error!("integration tests require \"mock_impl\" feature");

fn mock(conv: Vec<(Actor, ActionData)>) -> Connection {
    let io: Io = MockSocket::new(conv).into();
    Connection::from(io)
}

fn mock_no_shutdown(conv: Vec<(Actor, ActionData)>) -> Connection {
    let io: Io = MockSocket::new_no_check_shutdown(conv).into();
    Connection::from(io)
}

fn server_id() -> ClientIdentity {
    ClientIdentity::Domain("they.test".parse().unwrap())
}

fn client_id() -> ClientIdentity {
    ClientIdentity::Domain("me.test".parse().unwrap())
}


#[test]
fn test_ehlo_cmd() {
    let con = mock(vec![
        (Client,  Lines(vec!["EHLO me.test"])),
        (Server,  Lines(vec!["220-they.test greets you", "220-SMTPUTF8", "220 XBLA sSpecial"])),
    ]);

    let fut = con
        .send(command::Ehlo::new(client_id()))
        .map(|(con, result)| match result {
            Ok(_) => con,
            Err(e) => panic!("unexpected ehlo failed: {:?}", e)
        })
        .map_err(|err| -> () { panic!("unexpected error: {:?}", err) });

    let con = fut.wait().unwrap();
    {
        assert!(con.has_capability("SMTPUTF8"));
        assert!(con.has_capability("XBLA"));
        let params = con.ehlo_data().unwrap().get_capability_params("XBLA").unwrap();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], "sSpecial");
        assert_ne!(params[0], "sspecial");
    }

    con.shutdown().wait().unwrap();
}

#[test]
fn test_reset_cmd() {
    let con = mock_no_shutdown(vec![
        (Client,  Lines(vec!["RSET"])),
        (Server,  Lines(vec!["420 server messed up"])),
    ]);

    let fut = con
        .send(command::Reset)
        .map(|(con, result)| match result {
            Ok(r) => panic!("unexpected reset did not fail: {:?}", r),
            Err(e) => panic!("unexpected reset errord: {:?}", e)
        });

    let res = fut.wait();

    assert!(res.is_err());
}

#[test]
fn test_data_cmd() {
    //setup connection
    unimplemented!()
}

#[test]
fn test_mail_cmd() {
    //setup connection
    unimplemented!()
}

#[test]
fn test_recipient_cmd() {
    //setup connection
    unimplemented!()
}