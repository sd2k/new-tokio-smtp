use futures::Future;

use new_tokio_smtp::{command, ClientId};
use new_tokio_smtp::mock::{ActionData, Actor};
use self::ActionData::*;
use self::Actor::*;

use super::mock;

fn client_id() -> ClientId {
    ClientId::Domain("me.test".parse().unwrap())
}

#[test]
fn normalified_ehlo_response() {
    let con = mock(vec![
        (Client, Lines(vec!["EHLO me.test"])),
        (
            Server,
            Lines(vec![
                "250-example.de",
                "250-PIPELINING",
                "250-SIZE 90000000",
                "250-VRFY",
                "250-ETRN",
                "250-STARTTLS",
                "250-ENHANCEDSTATUSCODES",
                "250-8BITMIME",
                "250 DSN",
            ]),
        ),
    ]);

    let fut = con
        .send(command::Ehlo::new(client_id()))
        .map(|(con, result)| match result {
            Ok(_) => con,
            Err(e) => panic!("unexpected ehlo failed: {:?}", e),
        })
        .map_err(|err| -> () { panic!("unexpected error: {:?}", err) });

    let con = fut.wait().unwrap();
    {
        assert!(con.has_capability("PIPELINING"));
        assert!(con.has_capability("SIZE"));
        assert!(con.has_capability("VRFY"));
        assert!(con.has_capability("ETRN"));
        assert!(con.has_capability("STARTTLS"));
        assert!(con.has_capability("ENHANCEDSTATUSCODES"));
        assert!(con.has_capability("8BITMIME"));
        assert!(con.has_capability("DSN"));
        let params = con
            .ehlo_data()
            .unwrap()
            .get_capability_params("SIZE")
            .unwrap();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], "90000000");
    }

    con.shutdown().wait().unwrap();
}

