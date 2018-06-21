extern crate futures;
extern crate tokio;
extern crate new_tokio_smtp;
#[macro_use]
extern crate vec1;
extern crate rpassword;

use std::io::{stdin, stdout, Write};

use futures::stream::{self, Stream};
use futures::future::{lazy, Future};
use new_tokio_smtp::error::GeneralError;
use new_tokio_smtp::{
    command, Connection, ConnectionConfig,
    Security, ClientIdentity, Domain
};
use new_tokio_smtp::send_mail::{
    Mail, EncodingRequirement,
    MailAddress, MailEnvelop,
};

struct Request {
    config: ConnectionConfig<command::AuthPlain>,
    mails: Vec<MailEnvelop>
}

fn main() {
    let Request { config, mails } = read_request();

    println!("[now starting tokio]");
    tokio::run(lazy(move || {
        let mails = stream::iter_ok::<_, GeneralError>(mails);
        println!("[start connect_send_quit]");
        Connection::connect_send_quit(config, mails)
            .and_then(|results| {
                results.for_each(|result| {
                    if let Err(err) = result {
                        println!("[sending mail failed]: {}", err);
                    } else {
                        println!("[successfully send mail]")
                    }
                    Ok(())
                })
                // will be gone once `!` is stable
                .map_err(|_| unreachable!())
            })
            .or_else(|conerr| {
                println!("[connecting failed]: {}", conerr);
                Ok(())
            })
    }))
}


fn read_request() -> Request {

    println!("preparing to send mail with ethereal.email");
    let sender = read_email();
    let passwd = read_password();

    let config: ConnectionConfig<_> = ConnectionConfig {
        addr: "178.32.207.71:587".parse().unwrap(),
        security: Security::StartTls(Domain::from_str_unchecked("ethereal.email").into()),
        client_id: ClientIdentity::localhost(),
        auth_cmd: command::AuthPlain::from_username(sender.clone(), passwd).unwrap()
    };

    // the from_unchecked normally can be used if we know the address is valid
    // a mail address parser will be added at some point in the future
    let send_to = MailAddress::from_str_unchecked("invalid@test.test");

    // using string fmt to crate mails IS A
    // REALLY BAD IDEA there are a ton of ways
    // this can go wrong, so don't do this in
    // practice, use some library to crate mails
    let raw_mail = format!(concat!(
        "Date: Thu, 14 Jun 2018 11:22:18 +0000\r\n",
        "From: You <{}>\r\n",
        //ethereal doesn't delivers any mail so it's fine
        "To: Invalid <{}>\r\n",
        "Subject: I am spam?\r\n",
        "\r\n",
        "...\r\n"
    ), sender.as_str(), send_to.as_str());

    // this normally adapts to a higher level abstraction
    // of mail then this crate provides
    let mail_data = Mail::new(EncodingRequirement::None, raw_mail.to_owned());

    let mail = MailEnvelop::new(sender, vec1![ send_to ], mail_data);

    Request {
        config,
        mails: vec![ mail ]
    }
}

fn read_email() -> MailAddress {
    let stdout = stdout();
    let mut handle = stdout.lock();
    write!(handle, "enter ethereal.email mail address\n[Note mail is not validated in this example]: ")
        .unwrap();
    handle.flush().unwrap();

    let mut line = String::new();
    stdin().read_line(&mut line).unwrap();
    MailAddress::from_str_unchecked(line.trim())
}

fn read_password() -> String {
    rpassword::prompt_password_stdout("password: ").unwrap()
}