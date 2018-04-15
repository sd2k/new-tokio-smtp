use std::{io as std_io};
use std::collections::HashMap;

use bytes::BufMut;
use futures::Future;

use ::{
    Domain, EhloData, SyntaxError, EhloParam,
    Cmd, Connection, CmdFuture, Io, Response, ClientIdentity
};


#[derive(Debug, Clone)]
pub struct Ehlo {
    identity: ClientIdentity
}

impl Ehlo {

    pub fn new(identity: ClientIdentity) -> Self {
        Ehlo { identity }
    }

    pub fn identity(&self) -> &ClientIdentity {
        &self.identity
    }
}

impl From<ClientIdentity> for Ehlo {
    fn from(identity: ClientIdentity) -> Self {
        Ehlo { identity }
    }
}

impl Into<ClientIdentity> for Ehlo {
    fn into(self) -> ClientIdentity {
        self.identity
    }
}

impl Cmd for Ehlo {

    fn exec(self, con: Connection) -> CmdFuture {
        let mut io = con.into_inner();

        let str_me = match *self.identity() {
            ClientIdentity::Domain(ref domain) => domain.as_str(),
            ClientIdentity::AddressLiteral(ref addr_lit) => addr_lit.as_str()
        };

        {
            //7 == "EHLO ".len() + "\r\n".len()
            let out = io.out_buffer(7 + str_me.len());
            out.put("EHLO ");
            out.put(str_me);
            out.put("\r\n");
        }

        let fut = io
            .flush()
            .and_then(Io::parse_response)
            .and_then(|(mut io, result)| match result {
                Err(response) => Ok((Connection::from(io), Err(response))),
                Ok(response) => {
                    let ehlo = parse_ehlo_response(&response)
                        .map_err(|err| std_io::Error::new(std_io::ErrorKind::Other, err))?;

                    io.set_ehlo_data(ehlo);
                    Ok((Connection::from(io), Ok(response)))
                }
            });

        Box::new(fut)
    }
}

fn parse_ehlo_response(response: &Response) -> Result<EhloData, SyntaxError> {
    let lines = response.msg();
    let first = lines.first().expect("response with 0 lines should not");
    //UNWRAP_SAFE: Split has at last one entry
    let domain: Domain = first.split(" ").next().unwrap().parse()?;
    let mut caps = HashMap::new();

    for line in lines[1..].iter() {
        let mut parts = line.split(" ");
        //UNWRAP_SAFE: Split has at last one entry
        let capability = parts.next().unwrap().parse()?;
        let params = parts.map(|part| part.parse()).collect::<Result<Vec<EhloParam>, _>>()?;
        caps.insert(capability, params);
    }

    Ok(EhloData::new(domain, caps))
}


#[cfg(test)]
mod test {

    mod parse_ehlo_response {
        use ::Response;
        use ::response::codes::OK;
        use super::super::parse_ehlo_response;

        #[test]
        fn simple_case() {
            let response = Response::new(OK, vec!["1aim.test".to_owned()]);
            let ehlo_data = parse_ehlo_response(&response).unwrap();

            assert_eq!(ehlo_data.domain(), "1aim.test");
            assert!(ehlo_data.capability_map().is_empty());
        }

        #[test]
        fn allow_greeting() {
            let response = Response::new(OK, vec!["1aim.test says hy".to_owned()]);
            let ehlo_data = parse_ehlo_response(&response).unwrap();

            assert_eq!(ehlo_data.domain(), "1aim.test");
            assert!(ehlo_data.capability_map().is_empty());
        }

        #[test]
        fn can_have_capabilities() {
            let response = Response::new(OK, vec![
                "1aim.test says hy".to_owned(),
                "SMTPUTF8".to_owned(),
                "MIME8".to_owned(),
            ]);
            let ehlo_data = parse_ehlo_response(&response).unwrap();

            assert_eq!(ehlo_data.domain(), "1aim.test");
            assert!(ehlo_data.has_capability("SMTPUTF8"));
            assert!(ehlo_data.get_capability_params("SMTPUTF8").unwrap().is_empty());
            assert!(ehlo_data.has_capability("MIME8"));
            assert!(ehlo_data.get_capability_params("MIME8").unwrap().is_empty());
            assert_eq!(ehlo_data.capability_map().len(), 2)
        }

        #[test]
        fn capabilities_can_have_parameter() {
            let response = Response::new(OK, vec![
                "1aim.test says hy".to_owned(),
                "X-NOT-A-ROBOT ENABLED".to_owned(),
            ]);
            let ehlo_data = parse_ehlo_response(&response).unwrap();

            assert_eq!(ehlo_data.domain(), "1aim.test");
            assert!(ehlo_data.has_capability("X-NOT-A-ROBOT"));
            let params = ehlo_data.get_capability_params("X-NOT-A-ROBOT").unwrap();
            assert_eq!(params.len(), 1);
            assert_eq!(params[0], "ENABLED");
        }
    }
}