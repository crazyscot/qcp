//! Handler for a single incoming stream on a connection
// (c) 2024 Ross Younger

use crate::protocol::common::{
    ProtocolMessage as _, ReceivingStream, SendReceivePair, SendingStream,
};
use crate::protocol::session::Command;

use tracing::{trace, trace_span};

pub(super) async fn handle_stream<W, R>(mut sp: SendReceivePair<W, R>) -> anyhow::Result<()>
where
    R: ReceivingStream + 'static, // AsyncRead + Unpin + Send,
    W: SendingStream + 'static,   // AsyncWrite + Unpin + Send,
{
    use crate::session;
    trace!("reading command");
    let packet = Command::from_reader_async_framed(&mut sp.recv).await?;

    let mut handler = match packet {
        Command::Get(args) => {
            trace_span!("SERVER:GET", filename = args.filename.clone());
            session::Get::boxed(sp, Some(args))
        }
        Command::Put(args) => {
            trace_span!("SERVER:PUT", filename = args.filename.clone());
            session::Put::boxed(sp, Some(args))
        }
    };

    handler.handle().await
}
