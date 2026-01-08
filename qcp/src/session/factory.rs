//! Factory functions for creating session command implementations
// (c) 2025 Ross Younger

use crate::protocol::common::{ReceivingStream, SendReceivePair, SendingStream};
use crate::protocol::control::Compatibility;
use crate::protocol::session::{Command, Get2Args, GetArgs, Put2Args, PutArgs};

use super::SessionCommandImpl;
use super::{CreateDirectory, Get, Listing, Put, SetMetadata};

/// Span information for a command (used for tracing)
pub(crate) struct SpanInfo {
    pub name: &'static str,
    pub primary_arg: String,
}

/// Factory function to create the appropriate session command handler from a parsed command.
pub(crate) fn command_handler<S: SendingStream + 'static, R: ReceivingStream + 'static>(
    stream: SendReceivePair<S, R>,
    command: Command,
    compat: Compatibility,
) -> (Box<dyn SessionCommandImpl>, SpanInfo) {
    let (handler, span_info): (Box<dyn SessionCommandImpl>, SpanInfo) = match command {
        Command::Get(GetArgs { filename }) => (
            Get::boxed(
                stream,
                Some(Get2Args {
                    filename: filename.clone(),
                    options: vec![],
                }),
                compat,
            ),
            SpanInfo {
                name: "GET",
                primary_arg: filename,
            },
        ),
        Command::Get2(args) => {
            let filename = args.filename.clone();
            (
                Get::boxed(stream, Some(args), compat),
                SpanInfo {
                    name: "GET2",
                    primary_arg: filename,
                },
            )
        }
        Command::Put(PutArgs { filename }) => (
            Put::boxed(
                stream,
                Some(Put2Args {
                    filename: filename.clone(),
                    options: vec![],
                }),
                compat,
            ),
            SpanInfo {
                name: "PUT",
                primary_arg: filename,
            },
        ),
        Command::Put2(args) => {
            let filename = args.filename.clone();
            (
                Put::boxed(stream, Some(args), compat),
                SpanInfo {
                    name: "PUT2",
                    primary_arg: filename,
                },
            )
        }
        Command::CreateDirectory(args) => {
            let dir_name = args.dir_name.clone();
            (
                CreateDirectory::boxed(stream, Some(args), compat),
                SpanInfo {
                    name: "MKDIR",
                    primary_arg: dir_name,
                },
            )
        }
        Command::SetMetadata(args) => {
            let path = args.path.clone();
            (
                SetMetadata::boxed(stream, Some(args), compat),
                SpanInfo {
                    name: "SETMETA",
                    primary_arg: path,
                },
            )
        }
        Command::List(args) => {
            let path = args.path.clone();
            (
                Listing::boxed(stream, Some(args), compat),
                SpanInfo {
                    name: "LS",
                    primary_arg: path,
                },
            )
        }
    };
    (handler, span_info)
}
