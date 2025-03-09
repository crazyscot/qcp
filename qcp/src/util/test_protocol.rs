//! Test helpers for functions dealing with on-wire protocols
// (c) 2025 Ross Younger

use tokio_pipe::{PipeRead, PipeWrite};

use crate::protocol::common::{ReceivingStream, SendReceivePair, SendingStream};

pub(crate) type TestStreamPair = SendReceivePair<PipeWrite, PipeRead>;

impl ReceivingStream for PipeRead {}
impl SendingStream for PipeWrite {}

/// In order to test a streaming function we need a bi-directional stream.
/// A single call to `tokio_pipe::pipe()` isn't useful by itself, as it returns
/// a writer which the corresponding reader accesses.
/// We need two such pipes; each side of the streaming function under test takes one
/// such reader and the _opposite_ writer.
pub(crate) fn test_plumbing() -> (TestStreamPair, TestStreamPair) {
    let p1 = tokio_pipe::pipe().unwrap();
    let p2 = tokio_pipe::pipe().unwrap();
    let r1 = (p1.1, p2.0).into();
    let r2 = (p2.1, p1.0).into();
    (r1, r2)
}
