use std::{collections::HashMap, io as std_io};

use bytes::BufMut;
use futures::Future;
#[cfg(feature = "log")]
use log_facade::warn;

use crate::{
    error::MissingCapabilities, Capability, ClientId, Cmd, Domain, EhloData, EhloParam, ExecFuture,
    Io, Response, SyntaxError, SyntaxErrorHandling,
};

#[derive(Debug, Clone)]
pub struct Ehlo {
    identity: ClientId,
    //FIXME: Move this into the connection as a form of
    //  "connection runtime settings" or similar. But this
    //  should wait until moving to async/await.
    syntax_error_handling: SyntaxErrorHandling,
}

impl Ehlo {
    pub fn new(identity: ClientId) -> Self {
        Ehlo {
            identity,
            syntax_error_handling: Default::default(),
        }
    }

    pub fn with_syntax_error_handling(mut self, method: SyntaxErrorHandling) -> Self {
        self.syntax_error_handling = method;
        self
    }

    pub fn syntax_error_handling(&self) -> &SyntaxErrorHandling {
        &self.syntax_error_handling
    }

    pub fn identity(&self) -> &ClientId {
        &self.identity
    }
}

impl From<ClientId> for Ehlo {
    fn from(identity: ClientId) -> Self {
        Ehlo::new(identity)
    }
}

impl Into<ClientId> for Ehlo {
    fn into(self) -> ClientId {
        self.identity
    }
}

impl Cmd for Ehlo {
    fn check_cmd_availability(&self, _caps: Option<&EhloData>) -> Result<(), MissingCapabilities> {
        Ok(())
    }

    fn exec(self, mut io: Io) -> ExecFuture {
        let error_on_bad_ehlo_capabilities =
            self.syntax_error_handling() == &SyntaxErrorHandling::Strict;
        let str_me = match *self.identity() {
            ClientId::Domain(ref domain) => domain.as_str(),
            ClientId::AddressLiteral(ref addr_lit) => addr_lit.as_str(),
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
            //TODO ctx_and_then
            .and_then(move |(mut io, result)| match result {
                Err(response) => Ok((io, Err(response))),
                Ok(response) => {
                    let ehlo = parse_ehlo_response(&response, error_on_bad_ehlo_capabilities)
                        .map_err(|err| std_io::Error::new(std_io::ErrorKind::Other, err))?;

                    io.set_ehlo_data(ehlo);
                    Ok((io, Ok(response)))
                }
            });

        Box::new(fut)
    }
}

fn parse_ehlo_response(
    response: &Response,
    error_on_bad_ehlo_capabilities: bool,
) -> Result<EhloData, SyntaxError> {
    let lines = response.msg();
    let first = lines.first().expect("response with 0 lines should not");
    //UNWRAP_SAFE: Split has at last one entry
    let domain: Domain = first.split(" ").next().unwrap().parse()?;
    let mut caps = HashMap::new();

    for line in lines[1..].iter() {
        match parse_capability_in_ehlo_response(line) {
            Ok((cap, params)) => {
                caps.insert(cap, params);
            }
            Err(err) if error_on_bad_ehlo_capabilities => {
                return Err(err);
            }
            Err(_err) => {
                #[cfg(feature = "log")]
                warn!("Parsing Server EHLO response partially failed: {}", _err);
            }
        }
    }

    Ok(EhloData::new(domain, caps))
}

fn parse_capability_in_ehlo_response(
    line: &str,
) -> Result<(Capability, Vec<EhloParam>), SyntaxError> {
    let mut parts = line.split(" ");
    //UNWRAP_SAFE: Split has at last one entry
    let capability = parts.next().unwrap().parse()?;
    let params = parts
        .map(|part| part.parse())
        .collect::<Result<Vec<EhloParam>, _>>()?;
    Ok((capability, params))
}

#[cfg(test)]
mod test {

    mod parse_ehlo_response {
        use super::super::parse_ehlo_response;
        use crate::{response::codes::OK, Response};

        #[test]
        fn simple_case() {
            let response = Response::new(OK, vec!["1aim.test".to_owned()]);
            let ehlo_data = parse_ehlo_response(&response, true).unwrap();

            assert_eq!(ehlo_data.domain(), "1aim.test");
            assert!(ehlo_data.capability_map().is_empty());
        }

        #[test]
        fn allow_greeting() {
            let response = Response::new(OK, vec!["1aim.test says hy".to_owned()]);
            let ehlo_data = parse_ehlo_response(&response, true).unwrap();

            assert_eq!(ehlo_data.domain(), "1aim.test");
            assert!(ehlo_data.capability_map().is_empty());
        }

        #[test]
        fn can_have_capabilities() {
            let response = Response::new(
                OK,
                vec![
                    "1aim.test says hy".to_owned(),
                    "SMTPUTF8".to_owned(),
                    "MIME8".to_owned(),
                ],
            );
            let ehlo_data = parse_ehlo_response(&response, true).unwrap();

            assert_eq!(ehlo_data.domain(), "1aim.test");
            assert!(ehlo_data.has_capability("SMTPUTF8"));
            assert!(ehlo_data
                .get_capability_params("SMTPUTF8")
                .unwrap()
                .is_empty());
            assert!(ehlo_data.has_capability("MIME8"));
            assert!(ehlo_data.get_capability_params("MIME8").unwrap().is_empty());
            assert_eq!(ehlo_data.capability_map().len(), 2)
        }

        #[test]
        fn capabilities_can_have_parameter() {
            let response = Response::new(
                OK,
                vec![
                    "1aim.test says hy".to_owned(),
                    "X-NOT-A-ROBOT ENABLED".to_owned(),
                ],
            );
            let ehlo_data = parse_ehlo_response(&response, true).unwrap();

            assert_eq!(ehlo_data.domain(), "1aim.test");
            assert!(ehlo_data.has_capability("X-NOT-A-ROBOT"));
            let params = ehlo_data.get_capability_params("X-NOT-A-ROBOT").unwrap();
            assert_eq!(params.len(), 1);
            assert_eq!(params[0], "ENABLED");
        }

        #[test]
        fn ignore_malformed_capabilities() {
            let response = Response::new(
                OK,
                vec![
                    "1aim.test says hy".to_owned(),
                    "X-NOT-\0-ROBOT".to_owned(),
                    "X-NOT-A-ROBOT".to_owned(),
                ],
            );
            let _err = parse_ehlo_response(&response, true).unwrap_err();
            let ehlo_data = parse_ehlo_response(&response, false).unwrap();

            assert_eq!(ehlo_data.domain(), "1aim.test");
            assert_eq!(ehlo_data.capability_map().len(), 1);
            assert!(ehlo_data.has_capability("X-NOT-A-ROBOT"));
        }

        #[test]
        fn issue_05_a() {
            let response = Response::new(
                OK,
                vec![
                    "example.de ESMTP Postfix (Debian/GNU)".to_owned(),
                    "example.de".to_owned(),
                    "PIPELINING".to_owned(),
                    "SIZE 90000000".to_owned(),
                    "VRFY".to_owned(),
                    "ETRN".to_owned(),
                    "STARTTLS".to_owned(),
                    "ENHANCEDSTATUSCODES".to_owned(),
                    "8BITMIME".to_owned(),
                    "DSN".to_owned(),
                ],
            );
            let _ehlo_data = parse_ehlo_response(&response, false).unwrap();
            let _err = parse_ehlo_response(&response, true).unwrap_err();
        }

        #[test]
        fn issue_05_b() {
            let response = Response::new(
                OK,
                vec![
                    "example.de ESMTP Postfix (Debian/GNU)".to_owned(),
                    "PIPELINING".to_owned(),
                    "SIZE 90000000".to_owned(),
                    "VRFY".to_owned(),
                    "ETRN".to_owned(),
                    "STARTTLS".to_owned(),
                    "ENHANCEDSTATUSCODES".to_owned(),
                    "8BITMIME".to_owned(),
                    "DSN".to_owned(),
                ],
            );
            let _ehlo_data = parse_ehlo_response(&response, false).unwrap();
            let _ehlo_data = parse_ehlo_response(&response, true).unwrap();
        }

        #[test]
        fn issue_05_c() {
            let response = Response::new(
                OK,
                vec![
                    "example.de".to_owned(),
                    "PIPELINING".to_owned(),
                    "SIZE 90000000".to_owned(),
                    "VRFY".to_owned(),
                    "ETRN".to_owned(),
                    "STARTTLS".to_owned(),
                    "ENHANCEDSTATUSCODES".to_owned(),
                    "8BITMIME".to_owned(),
                    "DSN".to_owned(),
                ],
            );
            let _ehlo_data = parse_ehlo_response(&response, true).unwrap();
        }
    }
}
