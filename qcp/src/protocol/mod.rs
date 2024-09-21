// qcp::protocol

pub mod control;
pub mod session;

pub type RawStreamPair = (quinn::SendStream, quinn::RecvStream);

/// Syntactic sugar type (though I expect some might call it salt)
pub struct StreamPair {
    pub send: quinn::SendStream,
    pub recv: quinn::RecvStream,
    // The underlying Send/Recv stream objects have Drop handlers which do the Right Thing.
}

impl From<RawStreamPair> for StreamPair {
    fn from(value: RawStreamPair) -> Self {
        Self {
            send: value.0,
            recv: value.1,
        }
    }
}
