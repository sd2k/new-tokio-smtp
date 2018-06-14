#![allow(non_snake_case)]


use new_tokio_smtp::{
    command,
    ClientIdentity,
};

use new_tokio_smtp::mock::{
    ActionData, Actor
};

use self::Actor::*;
use self::ActionData::*;

use super::{mock, mock_no_shutdown};


//fn server_id() -> ClientIdentity {
//    ClientIdentity::Domain("they.test".parse().unwrap())
//}

fn client_id() -> ClientIdentity {
    ClientIdentity::Domain("me.test".parse().unwrap())
}


mod Ehlo {
    use futures::Future;
    use super::*;


    #[test]
    fn parsed_response_into_ehlo_data() {
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

}

mod Reset {
    use futures::Future;
    use super::*;

    #[test]
    fn turns_unexpected_codes_into_failure() {
        let con = mock_no_shutdown(vec![
            (Client,  Lines(vec!["RSET"])),
            (Server,  Lines(vec!["420 server messed up"])),
        ]);

        let fut = con
            .send(command::Reset)
            .map(|(_con, result)| match result {
                Ok(r) => panic!("unexpected reset did not fail: {:?}", r),
                Err(e) => panic!("unexpected reset errord: {:?}", e)
            });

        let res = fut.wait();

        assert!(res.is_err());
    }
}

mod Data {
    //TODO test
}

mod Mail {
    //TODO test
}

mod Recipient {
    //todo test
}
