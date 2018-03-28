use std::{io as std_io};
use std::collections::HashMap;

use bytes::BufMut;
use futures::Future;

use ::{
    Domain, AddressLiteral, EhloData, SyntaxError, EhloParam,
    Cmd, Connection, CmdFuture, Io, Response
};


#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum Ehlo {
    Domain(Domain),
    AddressLiteral(AddressLiteral)
}

impl Cmd for Ehlo {

    fn exec(self, con: Connection) -> CmdFuture {
        let (mut io, _ehlo) = con.destruct();

        let str_me: String = match self {
            Ehlo::Domain(domain) => domain.into(),
            Ehlo::AddressLiteral(addrl) => addrl.into()
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
            .and_then(|(io, result)| match result {
                Err(response) => Ok((Connection::from((io, None)), Err(response))),
                Ok(response) => {
                    let ehlo = parse_ehlo_response(&response)
                        .map_err(|err| std_io::Error::new(std_io::ErrorKind::Other, err))?;

                    Ok((Connection::from((io, Some(ehlo))), Ok(response)))
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