//! Test helpers for functions dealing with on-wire protocols
// (c) 2025 Ross Younger

use crate::protocol::common::{ProtocolMessage, ReceivingStream, SendReceivePair, SendingStream};

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use either::{Either, Left, Right};
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
pub(crate) fn new_test_plumbing() -> (TestStreamPair, TestStreamPair) {
    let p1 = simplex(STREAM_BUFFER_SIZE);
    let p2 = simplex(STREAM_BUFFER_SIZE);
    let r1 = (p1.1, p2.0).into();
    let r2 = (p2.1, p1.0).into();
    (r1, r2)
}

/// Attempts to read and deserialise a protocol object from a stream
/// (ostensibly the receiving end of a pipe pair, but we're agnostic about it).
///
/// This is moderately tricky as the pipe is temporarily borrowed by a future, but by doing it
/// in a function the future is guaranteed to release the borrow on return.
///
/// ## Arguments
/// * `pipe`: Where to read from
/// * `other_fut`: A future which has to do work in order for `pipe` to produce the object,
///   but which we expect will not complete before it.
///
/// ## Return values
/// If the protocol object was read successfully, returns `Left<object>`.
/// If the other future completed first, returns `Right<return from the future>`.
///
pub(crate) async fn read_from_stream<T, R, F>(
    pipe: &mut R,
    other_fut: &mut Pin<Box<F>>,
) -> Either<Result<T>, <F as futures_util::Future>::Output>
where
    T: ProtocolMessage,
    R: ReceivingStream,
    F: Future + ?Sized,
{
    let obj_fut = T::from_reader_async_framed(pipe);
    tokio::pin!(obj_fut);
    tokio::select! {
        o = obj_fut => Left(o),
        v = other_fut => Right(v),
    }
}
