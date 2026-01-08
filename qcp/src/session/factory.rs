//! Factory functions for creating session command implementations
// (c) 2025 Ross Younger

use crate::protocol::common::{ReceivingStream, SendReceivePair, SendingStream};
use crate::protocol::control::Compatibility;
use crate::protocol::session::{
    Command, CommandParam, Get2Args, GetArgs, ListArgs, Put2Args, PutArgs,
};

use crate::Parameters;
use crate::client::CopyJobSpec;

use super::SessionCommandImpl;
use super::handler::{
    CreateDirectoryHandler, GetHandler, ListingHandler, PutHandler, SessionCommand,
    SetMetadataHandler,
};

/// Span information for a command (used for tracing)
pub(crate) struct SpanInfo {
    pub name: &'static str,
    pub primary_arg: String,
}

/// Phase of the transfer operation, determining which command to create
#[derive(Debug, Copy, Clone)]
pub(crate) enum TransferPhase {
    /// Pre-transfer phase: list directory contents (remote source only)
    Pre,
    /// Transfer phase: GET, PUT, or CREATE_DIRECTORY
    Transfer,
    /// Post-transfer phase: set metadata on remote destination (remote dest, preserve mode, directory only)
    Post,
}

/// Factory function to create the appropriate client-side command sender from a copy job spec.
///
/// This centralizes the logic for determining which command to send based on the copy job spec
/// and transfer phase, replacing the scattered match statements in main_loop.rs.
pub(crate) fn client_sender<S: SendingStream + 'static, R: ReceivingStream + 'static>(
    stream: SendReceivePair<S, R>,
    copy_spec: &CopyJobSpec,
    phase: TransferPhase,
    compat: Compatibility,
    params: &Parameters,
) -> (Box<dyn SessionCommandImpl>, SpanInfo) {
    match phase {
        TransferPhase::Pre => {
            // Pre-transfer: list remote directory
            let path = copy_spec.source.filename.clone();
            let mut options = vec![];
            if params.recurse {
                options.push(CommandParam::Recurse.into());
            }
            (
                SessionCommand::boxed(
                    stream,
                    Some(ListArgs {
                        path: path.clone(),
                        options,
                    }),
                    compat,
                    ListingHandler,
                ),
                SpanInfo {
                    name: "LIST",
                    primary_arg: path,
                },
            )
        }
        TransferPhase::Transfer => {
            if copy_spec.source.user_at_host.is_some() {
                // Remote source: GET
                let mut args = Get2Args::default();
                args.filename.clone_from(&copy_spec.source.filename);
                if copy_spec.preserve {
                    args.options.push(CommandParam::PreserveMetadata.into());
                }
                (
                    SessionCommand::boxed(stream, Some(args), compat, GetHandler),
                    SpanInfo {
                        name: "GETx",
                        primary_arg: copy_spec.source.filename.clone(),
                    },
                )
            } else if copy_spec.directory {
                // Local source, directory: MKDIR
                (
                    SessionCommand::boxed(stream, None, compat, CreateDirectoryHandler),
                    SpanInfo {
                        name: "MKDIR",
                        primary_arg: copy_spec.destination.filename.clone(),
                    },
                )
            } else {
                // Local source, file: PUT
                (
                    SessionCommand::boxed(stream, None, compat, PutHandler),
                    SpanInfo {
                        name: "PUTx",
                        primary_arg: copy_spec.source.filename.clone(),
                    },
                )
            }
        }
        TransferPhase::Post => {
            // Post-transfer: set metadata on remote directory
            let destination_filename = copy_spec.destination.filename.clone();
            (
                SessionCommand::boxed(stream, None, compat, SetMetadataHandler),
                SpanInfo {
                    name: "SETMETA",
                    primary_arg: destination_filename,
                },
            )
        }
    }
}

/// Factory function to create the appropriate session command handler from a parsed command.
pub(crate) fn command_handler<S: SendingStream + 'static, R: ReceivingStream + 'static>(
    stream: SendReceivePair<S, R>,
    command: Command,
    compat: Compatibility,
) -> (Box<dyn SessionCommandImpl>, SpanInfo) {
    let (handler, span_info): (Box<dyn SessionCommandImpl>, SpanInfo) = match command {
        Command::Get(GetArgs { filename }) => (
            SessionCommand::boxed(
                stream,
                Some(Get2Args {
                    filename: filename.clone(),
                    options: vec![],
                }),
                compat,
                GetHandler,
            ),
            SpanInfo {
                name: "GET",
                primary_arg: filename,
            },
        ),
        Command::Get2(args) => {
            let filename = args.filename.clone();
            (
                SessionCommand::boxed(stream, Some(args), compat, GetHandler),
                SpanInfo {
                    name: "GET2",
                    primary_arg: filename,
                },
            )
        }
        Command::Put(PutArgs { filename }) => (
            SessionCommand::boxed(
                stream,
                Some(Put2Args {
                    filename: filename.clone(),
                    options: vec![],
                }),
                compat,
                PutHandler,
            ),
            SpanInfo {
                name: "PUT",
                primary_arg: filename,
            },
        ),
        Command::Put2(args) => {
            let filename = args.filename.clone();
            (
                SessionCommand::boxed(stream, Some(args), compat, PutHandler),
                SpanInfo {
                    name: "PUT2",
                    primary_arg: filename,
                },
            )
        }
        Command::CreateDirectory(args) => {
            let dir_name = args.dir_name.clone();
            (
                SessionCommand::boxed(stream, Some(args), compat, CreateDirectoryHandler),
                SpanInfo {
                    name: "MKDIR",
                    primary_arg: dir_name,
                },
            )
        }
        Command::SetMetadata(args) => {
            let path = args.path.clone();
            (
                SessionCommand::boxed(stream, Some(args), compat, SetMetadataHandler),
                SpanInfo {
                    name: "SETMETA",
                    primary_arg: path,
                },
            )
        }
        Command::List(args) => {
            let path = args.path.clone();
            (
                SessionCommand::boxed(stream, Some(args), compat, ListingHandler),
                SpanInfo {
                    name: "LS",
                    primary_arg: path,
                },
            )
        }
    };
    (handler, span_info)
}
