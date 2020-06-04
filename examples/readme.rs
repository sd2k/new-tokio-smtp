extern crate futures;
extern crate new_tokio_smtp;
extern crate tokio;
#[macro_use]
extern crate vec1;
extern crate rpassword;

use std::io::{stdin, stdout, Write};

use futures::future::lazy;
use futures::stream::Stream;
use new_tokio_smtp::error::GeneralError;
use new_tokio_smtp::send_mail::{EncodingRequirement, Mail, MailAddress, MailEnvelop};
use new_tokio_smtp::{command, Connection, ConnectionConfig, Domain};

struct Request {
    config: ConnectionConfig<command::auth::Plain>,
    mails: Vec<MailEnvelop>,
}

fn main() {
    env_logger::init();

    let Request { config, mails } = read_request();
    // We only have iter map overhead because we
    // don't have a failable mail encoding step, which normally is required.
    let mails = mails
        .into_iter()
        .map(|m| -> Result<_, GeneralError> { Ok(m) });

    println!("[now starting tokio]");
    tokio::run(lazy(move || {
        println!("[start connect_send_quit]");
        Connection::connect_send_quit(config, mails)
            //Stream::for_each is design wise broken in futures v0.1
            .then(|result| Ok(result))
            .for_each(|result| {
                if let Err(err) = result {
                    println!("[sending mail failed]: {}", err);
                } else {
                    println!("[successfully send mail]")
                }
                Ok(())
            })
    }))
}

fn read_request() -> Request {
    println!("preparing to send mail with ethereal.email");
    let sender = read_email();
    let passwd = read_password();

    // The `from_unchecked` will turn into a `.parse()` in the future.
    let config = ConnectionConfig::builder(Domain::from_unchecked("smtp.ethereal.email"))
        .expect("resolving domain failed")
        .auth(
            command::auth::Plain::from_username(sender.clone(), passwd)
                .expect("username/password can not contain \\0 bytes"),
        )
        .build();

    // the from_unchecked normally can be used if we know the address is valid
    // a mail address parser will be added at some point in the future
    let send_to = MailAddress::from_unchecked("invalid@test.test");

    // using string fmt to crate mails IS A
    // REALLY BAD IDEA there are a ton of ways
    // this can go wrong, so don't do this in
    // practice, use some library to crate mails
    let raw_mail = format!(
        concat!(
            "Date: Thu, 14 Jun 2018 11:22:18 +0000\r\n",
            "From: You <{}>\r\n",
            //ethereal doesn't delivers any mail so it's fine
            "To: Invalid <{}>\r\n",
            "Subject: I am spam?\r\n",
            "\r\n",
            "...\r\n"
        ),
        sender.as_str(),
        send_to.as_str()
    );

    // this normally adapts to a higher level abstraction
    // of mail then this crate provides
    let mail_data = Mail::new(EncodingRequirement::None, raw_mail.to_owned());

    let mail = MailEnvelop::new(sender, vec1![send_to], mail_data);

    Request {
        config,
        mails: vec![mail],
    }
}

fn read_email() -> MailAddress {
    let stdout = stdout();
    let mut handle = stdout.lock();
    write!(
        handle,
        "enter ethereal.email mail address\n[Note mail is not validated in this example]: "
    )
    .unwrap();
    handle.flush().unwrap();

    let mut line = String::new();
    stdin().read_line(&mut line).unwrap();
    MailAddress::from_unchecked(line.trim())
}

fn read_password() -> String {
    rpassword::prompt_password_stdout("password: ").unwrap()
}
