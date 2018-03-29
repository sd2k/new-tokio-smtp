use ::io::MockStream;
use ::tokio::io::{AsyncRead, AsyncWrite};

pub enum Actor {
    Server,
    Client
}

pub enum ActionData {
    Lines(Vec<String>),
    Blob(Vec<u8>)
}


#[derive(Debug)]
pub struct MockSocket {
    fake_secure: bool,
    conversation: Vec<(Actor, ActionData)>
}

impl MockSocket {

    pub fn new(conversation: Vec<(Actor, ActionData)>) -> Self {
        MockSocket { conversation, fake_secure: false }
    }
}

impl AsyncRead for MockSocket {

}

impl AsyncWrite for MockSocket {

}

impl MockStream for MockSocket {
    fn is_secure(&self) -> bool {
        self.fake_secure
    }

    fn set_is_secure(&self, secure: bool) {
        self.fake_secure = secure;
    }
}