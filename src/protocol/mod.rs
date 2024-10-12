//! Protocol defininitions owned by qcp

pub mod control;
pub mod session;

/// Helper type definition (syntactic sugar)
pub(crate) type RawStreamPair = (quinn::SendStream, quinn::RecvStream);

/// Syntactic sugar type (though I expect some might call it salt)
#[derive(Debug)]
pub(crate) struct StreamPair {
    /// outbound data
    pub send: quinn::SendStream,
    /// inbound data
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
