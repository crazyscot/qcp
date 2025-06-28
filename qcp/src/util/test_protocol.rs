//! Test helpers for functions dealing with on-wire protocols
// (c) 2025 Ross Younger

use crate::protocol::common::{ReceivingStream, SendReceivePair, SendingStream};

use tokio::io::{ReadHalf, SimplexStream, WriteHalf, simplex};

type TestStreamPair = SendReceivePair<WriteHalf<SimplexStream>, ReadHalf<SimplexStream>>;

impl SendingStream for WriteHalf<SimplexStream> {}
impl ReceivingStream for ReadHalf<SimplexStream> {}

const STREAM_BUFFER_SIZE: usize = 4_096;

/// In order to test a streaming function we need a bi-directional stream.
/// A pipe isn't useful by itself, as it returns
/// a writer which the corresponding reader accesses.
/// We need two such pipes; each side of the streaming function under test takes one
/// such reader and the _opposite_ writer.
pub(crate) fn test_plumbing() -> (TestStreamPair, TestStreamPair) {
    let p1 = simplex(STREAM_BUFFER_SIZE);
    let p2 = simplex(STREAM_BUFFER_SIZE);
    let r1 = (p1.1, p2.0).into();
    let r2 = (p2.1, p1.0).into();
    (r1, r2)
}
