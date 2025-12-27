//! QCP session protocol definitions and helper types
// (c) 2024 Ross Younger
//!
//! The session protocol operates over a QUIC bidirectional stream.
//!
//! The protocol consists of [Command] and [Response] packets and helper structs.
//!
//! * Client ➡️ Server: (initiates QUIC stream)
//! * C ➡️ S : [Command] packet. This is an enum containing arguments needed by the selected command.
//! * S ➡️ C : [Response] packet
//! * Then they do whatever is appropriate for the command.
//!
//! The following commands are defined:
//! ### Get
//!
//! Retrieves a file from the remote.
//! * C ➡️ S: [GetArgs] _(within [Command])_
//! * S ➡️ C: [Response] . If the status within was not OK, the command does not proceed.
//! * S ➡️ C: [FileHeader], file data, [FileTrailer].
//!
//! After transfer, close the stream.
//!
//! Either side may close the stream mid-flow if it needs to abort the transfer.
//!
//! ### Put
//!
//! Sends a file to the remote.
//! * C ➡️ S: [PutArgs] _(within [Command])_, [FileHeader] _(see note!)_
//! * S ➡️ C: [Response] to the command.
//!   The server has already opened the destination file for writing, so has applied permission checks.
//!   If the status is not OK, the command does not proceed.
//! * C ➡️ S: file data, [FileTrailer].
//! * S ➡️ C: [Response] indicating transfer status
//!
//! _N.B. In versions 0.3.0 through to 0.3.3, the server's [Response] was sent between [PutArgs] and [FileHeader].
//!  This is a minor protocol refinement that improves reliability and testability without affecting compatibility._
//!
//! After transfer, close the stream.
//!
//! If the server needs to abort the transfer mid-flow, it may send a Response explaining why, then close the stream.
//!
//! # Wire encoding
//!
//! On the wire these are [BARE] messages.
//!
//! Note that serde_bare by default encodes enums on the wire as uints (rust `usize`),
//! ignoring any explicit discriminant!
//!
//! Unit enums (C-like) may be encoded with explicitly sized types (repr attribute) and using
//! their discriminant as the wire value, if derived from `Serialize_repr` or `Deserialize_repr`.
//!
//! # See also
//! [Common](super::common) protocol functions
//!
//! [quic]: https://quicwg.github.io/
//! [BARE]: https://www.ietf.org/archive/id/draft-devault-bare-11.html

mod command;
pub use command::*;
mod get_put;
pub use get_put::*;
mod misc_fs;
pub use misc_fs::*;
mod response;
pub use response::*;
mod file_transfer;
pub use file_transfer::*;

/// Convenient includes for session protocol building blocks
pub mod prelude {
    pub use super::{Command, CommandParam, MetadataAttr, Response, Status};
    pub use crate::protocol::prelude::*;
}
