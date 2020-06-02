//! Provides access to `Response`, `ResponseCode` and parsing parts (form impl `Cmd`'s)
/// response of a smtp server
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Response {
    code: ResponseCode,
    lines: Vec<String>,
}

impl Response {
    /// crate a new Response from a response code and a number of lines
    ///
    /// If lines is empty a single empty line will be pushed to the
    /// lines `Vec`.
    pub fn new(code: ResponseCode, mut lines: Vec<String>) -> Self {
        if lines.is_empty() {
            lines.push(String::new());
        }
        Response { code, lines }
    }

    /// true if the response code is unknown or indicates an error
    pub fn is_erroneous(&self) -> bool {
        self.code.is_erroneous()
    }

    /// return the response code
    pub fn code(&self) -> ResponseCode {
        self.code
    }

    /// returns the lines of the msg/payload
    ///
    /// this will have at last one line, throuhg
    /// this line might be empty
    pub fn msg(&self) -> &[String] {
        &self.lines
    }
}

/// The response code of used by smtp server.
//FIXME impl Display
//FIXME impl Debug which shows it as byte string, i.e. human readable
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct ResponseCode([u8; 3]);

impl ResponseCode {
    /// true if the code starts with `2`
    pub fn is_positive(&self) -> bool {
        self.0[0] == b'2'
    }

    /// true if the code starts with `3`
    pub fn is_intermediate(&self) -> bool {
        self.0[0] == b'3'
    }

    /// true if the code starts with `4`
    pub fn is_transient_failure(&self) -> bool {
        self.0[0] == b'4'
    }

    /// true if the code starts with `5`
    pub fn is_permanent_failure(&self) -> bool {
        self.0[0] == b'5'
    }

    /// true if the code doesn't start with `2` or `3`
    pub fn is_erroneous(&self) -> bool {
        !self.is_positive() && !self.is_intermediate()
    }

    /// The actual bytes returned as response code.
    ///
    /// This could be for example `*b'250'`. I.e. it's
    /// in ascii characters. It's *not* a triplet of the
    /// ascii characters converted to integer numbers!
    //FIXME rename to as_ascii_bytes
    pub fn as_byte_string(&self) -> [u8; 3] {
        self.0
    }
}

pub mod parser {
    use super::{Response, ResponseCode};

    use std::error::Error;
    use std::fmt::{self, Display};
    use std::str::{self, Utf8Error};

    #[derive(Debug, Clone)]
    pub enum ParseError {
        LineLength,
        CodeMsgSeparator,
        Utf8(Utf8Error),
        CodeFormat {
            kind: u8,
            category: u8,
            detail: u8,
        },
        Code {
            expected: ResponseCode,
            got: ResponseCode,
        },
    }

    impl Display for ParseError {
        fn fmt(&self, fter: &mut fmt::Formatter) -> fmt::Result {
            write!(fter, "{:?}", self)
        }
    }

    impl Error for ParseError {}

    pub struct ResponseLine {
        pub code: ResponseCode,
        pub last_line: bool,
        pub msg: String,
    }

    pub fn parse_line(line: &[u8]) -> Result<ResponseLine, ParseError> {
        if line.len() < 4 {
            return Err(ParseError::LineLength);
        }
        let (code, tail) = line.split_at(3);
        let (sep, msg) = tail.split_at(1);

        let code = parse_code(code[0], code[1], code[2])?;
        let last_line = parse_separator(sep[0])?;
        let msg = parse_msg(msg)?.to_owned();

        Ok(ResponseLine {
            code,
            last_line,
            msg,
        })
    }

    /// A non-struct response code parser, as long as the code is made of digits it accepts it
    ///
    /// The RFC 5321 grammar is actually a bit more strict, only
    /// allowing `b'2' ..= b'5'` for the kind, `b'0' ..= b'5'` for category and
    /// `b'0' ..= b'9'` for the detail part.
    ///
    /// The reason why this parser is less strict is following:
    ///
    /// 1. there are other RFC's extending SMTP some could (but probably shouldn't)
    ///    extend the error code
    /// 2. most use cases either check the kind or the whole error code, so having
    ///    other categories shouldn't be a problem _and_ wrt. the kind other kinds
    ///    can just be collapsed into `is_erroneous`
    /// 3. In the end it allows the command impl. to handle "unexpected" response
    ///    codes, while making it hardly any harder/more complex to handle the
    ///    response code for existing commands
    ///
    pub fn parse_code(kind: u8, category: u8, detail: u8) -> Result<ResponseCode, ParseError> {
        // there is hardly any reason to be super struct on the response code
        // so aslong as it's a number it's fine
        if kind.is_ascii_digit() && category.is_ascii_digit() && detail.is_ascii_digit() {
            //FIXME kind-b'0', category-b'0', detail-b'0'
            Ok(ResponseCode([kind, category, detail]))
        } else {
            Err(ParseError::CodeFormat {
                kind,
                category,
                detail,
            })
        }
    }

    fn parse_separator(sep: u8) -> Result<bool, ParseError> {
        let last_line = match sep {
            b' ' => true,
            b'-' => false,
            _ => return Err(ParseError::CodeMsgSeparator),
        };
        Ok(last_line)
    }

    fn parse_msg(msg: &[u8]) -> Result<&str, ParseError> {
        str::from_utf8(msg).map_err(ParseError::Utf8)
    }

    ///
    /// Ignores the `last_line` field in the iterator, the called is required to
    /// check if the last line (and no previous line) has the field set to `true`.
    ///
    /// # Panics
    ///
    /// Panics if the lines iterator does not return at last one line.
    ///
    pub fn response_from_parsed_lines<I>(lines: I) -> Result<Response, ParseError>
    where
        I: IntoIterator<Item = ResponseLine>,
    {
        let mut iter = lines.into_iter();
        let first = iter.next().expect("called with zero lines");
        let code = first.code;
        let mut messages = vec![first.msg];

        for line in iter {
            if code != line.code {
                return Err(ParseError::Code {
                    expected: code,
                    got: line.code,
                });
            }

            messages.push(line.msg);
        }

        Ok(Response {
            code,
            lines: messages,
        })
    }
}

/// Predefined Codes based on RFC 5321
///
/// All command documentation starting with "RFC XXX:" is directly quoted from the rfx XXX,
/// command documentation starting with "RFC XXX;" is not quoted.
/// In case of "RFC 5321:" the quotes come from Section 4.2.3.
pub mod codes {
    #[allow(non_snake_case)]
    use super::ResponseCode;

    /// RFC 5321: System status, or system help reply
    pub static STATUS_RESPONSE: ResponseCode = ResponseCode(*b"211");

    /// RFC 5321: Help message
    /// (Information on how to use the receiver or the meaning of a
    /// particular non-standard command; this reply is useful
    /// only to the human user)
    pub static HELP_RESPONSE: ResponseCode = ResponseCode(*b"214");

    /// RFC 5321: <domain> Service ready
    pub static READY: ResponseCode = ResponseCode(*b"220");

    /// RFC 5321: <domain> Service closing transmission channel
    pub static CLOSING_CHANNEL: ResponseCode = ResponseCode(*b"221");

    /// RFC 5321: Requested mail action okay, completed
    pub static OK: ResponseCode = ResponseCode(*b"250");

    /// RFC 5321: User not local; will forward to <forward-path> (See Section 3.4)
    pub static OK_NOT_LOCAL: ResponseCode = ResponseCode(*b"251");

    /// RFC 5321: Cannot VRFY user, but will accept message and attempt delivery
    /// (See Section 3.5.3)
    pub static OK_UNVERIFIED: ResponseCode = ResponseCode(*b"252");

    /// RFC 5321: Start mail input; end with <CRLF>.<CRLF>
    pub static START_MAIL_DATA: ResponseCode = ResponseCode(*b"354");

    /// RFC 5321: <domain> Service not available, closing transmission channel
    /// (This may be a reply to any command if the service knows it must
    /// shut down)
    pub static SERVICE_UNAVAILABLE: ResponseCode = ResponseCode(*b"421");

    /// RFC 5321: Requested mail action not taken: mailbox unavailable (e.g.,
    /// mailbox busy or temporarily blocked for policy reasons)
    pub static MAILBOX_TEMP_UNAVAILABLE: ResponseCode = ResponseCode(*b"450");

    /// RFC 5321: Requested action aborted: local error in processing
    pub static LOCAL_ERROR: ResponseCode = ResponseCode(*b"451");

    /// RFC 5321: Requested action not taken: insufficient system storage
    pub static INSUFFICIENT_SYSTEM: ResponseCode = ResponseCode(*b"452");

    /// RFC 5321: Server unable to accommodate parameters
    pub static UNABLE_TO_ACCOMMODATE_PARAMETERS: ResponseCode = ResponseCode(*b"455");

    /// RFC 5321: Syntax error, command unrecognized (This may include errors such
    /// as command line too long)
    pub static SYNTAX_ERROR: ResponseCode = ResponseCode(*b"500");

    /// RFC 5321: Syntax error in parameters or arguments
    pub static PARAM_SYNTAX_ERROR: ResponseCode = ResponseCode(*b"501");

    /// RFC 5321: Command not implemented (see Section 4.2.4)
    pub static COMMAND_UNIMPLEMENTED: ResponseCode = ResponseCode(*b"502");

    /// RFC 5321: Bad sequence of commands
    pub static BAD_COMMAND_SEQUENCE: ResponseCode = ResponseCode(*b"503");

    /// RFC 5321: Command parameter not implemented
    pub static PARAMETER_NOT_IMPLEMENTED: ResponseCode = ResponseCode(*b"504");

    /// RFC 7504: Server does not accept mail
    pub static SERVER_DOES_NOT_ACCEPT_MAIL: ResponseCode = ResponseCode(*b"521");

    /// RFC 5321: Requested action not taken: mailbox unavailable (e.g., mailbox
    /// not found, no access, or command rejected for policy reasons)
    pub static MAILBOX_UNAVAILABLE: ResponseCode = ResponseCode(*b"550");

    /// RFC 5321: User not local; please try <forward-path> (See Section 3.4)
    pub static USER_NOT_LOCAL: ResponseCode = ResponseCode(*b"551");

    /// RFC 5321: Requested mail action aborted: exceeded storage allocation
    pub static EXCEEDED_STORAGE_ALLOCATION: ResponseCode = ResponseCode(*b"552");

    /// RFC 5321: Requested action not taken: mailbox name not allowed (e.g.,
    /// mailbox syntax incorrect)
    pub static BAD_MAILBOX_NAME: ResponseCode = ResponseCode(*b"553");

    /// RFC 5321: Transaction failed (Or, in the case of a connection-opening
    /// response, "No SMTP service here")
    pub static TRANSACTION_FAILED: ResponseCode = ResponseCode(*b"554");

    /// RFC 5321: MAIL FROM/RCPT TO parameters not recognized or not implemented
    pub static PARAM_NOT_RECOGNIZED: ResponseCode = ResponseCode(*b"555");

    /// RFC 7504; like 521 but returned when a intermediate gateway knows a server
    ///  will return 521 when connecting, and therefore does decide not to connect
    ///  with it at all
    pub static TARGET_DOES_NOT_ACCEPT_MAIL: ResponseCode = ResponseCode(*b"556");
}
