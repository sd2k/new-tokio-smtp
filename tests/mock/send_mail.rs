use futures::Future;

use new_tokio_smtp::send_mail::{
    Mail, MailAddress, MailEnvelop,
    EncodingRequirement,
    ConSendMailExt
};
use new_tokio_smtp::mock::{ ActionData, Actor};


use self::Actor::*;
use self::ActionData::*;

use super::{mock, with_capability};

#[test]
fn creates_the_right_chain() {
    let con = mock(vec![
        (Client,  Lines(vec!["MAIL FROM:<t1@test.test>"])),
        (Server,  Lines(vec!["250 Ok"])),
        (Client,  Lines(vec!["RCPT TO:<t2@test.test>"])),
        (Server,  Lines(vec!["250 Ok"])),
        (Client,  Lines(vec!["DATA"])),
        (Server,  Lines(vec!["354 ..."])),
        (Client,  Blob(Vec::from("the data\r\n..stashed\r\n.\r\n".to_owned()))),
        (Server,  Lines(vec!["250 Ok"])),
        (Client,  Lines(vec!["QUIT"])),
        (Server,  Lines(vec!["250 Ok"])),
    ]);

    let envelop =
        MailEnvelop::new(
            MailAddress::from_str_unchecked("t1@test.test"),
            vec1![
                MailAddress::from_str_unchecked("t2@test.test"),
            ],
            Mail::new(EncodingRequirement::None, Vec::from("the data\r\n.stashed\r\n"))
        );

    con.send_mail(envelop)
        .and_then(|(con, _)| con.quit())
        .wait().unwrap();
}


#[test]
fn uses_smtputf8_for_internationalized_mail_addresses() {
    let con = mock(vec![
        (Client,  Lines(vec!["MAIL FROM:<tü1@test.test> SMTPUTF8"])),
        (Server,  Lines(vec!["250 Ok"])),
        (Client,  Lines(vec!["RCPT TO:<tü2@test.test>"])),
        (Server,  Lines(vec!["250 Ok"])),
        (Client,  Lines(vec!["DATA"])),
        (Server,  Lines(vec!["354 ..."])),
        (Client,  Blob(Vec::from("the data\r\n..stashed\r\n.\r\n".to_owned()))),
        (Server,  Lines(vec!["250 Ok"])),
        (Client,  Lines(vec!["QUIT"])),
        (Server,  Lines(vec!["250 Ok"])),
    ]);

    let con = with_capability(con, "SMTPUTF8");

    let envelop =
        MailEnvelop::new(
            MailAddress::from_str_unchecked("tü1@test.test"),
            vec1![
                MailAddress::from_str_unchecked("tü2@test.test"),
            ],
            Mail::new(EncodingRequirement::None, Vec::from("the data\r\n.stashed\r\n"))
        );

    con.send_mail(envelop)
        .and_then(|(con, _)| con.quit())
        .wait().unwrap();
}