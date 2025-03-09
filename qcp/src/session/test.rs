//! Test helpers specific to the session protcol
// (c) 2025 Ross Younger

use std::pin::Pin;
use std::{fmt::Debug, future::Future};

use anyhow::Result;
use either::{Either, Left, Right};

use crate::protocol::{
    common::{ProtocolMessage, ReceivingStream},
    session::Status,
};

/// Attempts to read and deserialise a protocol object from the receiving end of a pipe pair.
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
pub(super) async fn read_from_plumbing<T, R, F>(
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

impl From<anyhow::Error> for Status {
    fn from(e: anyhow::Error) -> Self {
        e.downcast::<Self>().expect("Expected a Status")
    }
}
impl<R: Debug> From<anyhow::Result<R>> for Status {
    fn from(r: anyhow::Result<R>) -> Self {
        Self::from(r.unwrap_err())
    }
}
