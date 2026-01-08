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
use super::{CreateDirectory, Get, Listing, Put, SetMetadata};

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
                Listing::boxed(
                    stream,
                    Some(ListArgs {
                        path: path.clone(),
                        options,
                    }),
                    compat,
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
                    Get::boxed(stream, Some(args), compat),
                    SpanInfo {
                        name: "GETx",
                        primary_arg: copy_spec.source.filename.clone(),
                    },
                )
            } else if copy_spec.directory {
                // Local source, directory: MKDIR
                (
                    CreateDirectory::boxed(stream, None, compat),
                    SpanInfo {
                        name: "MKDIR",
                        primary_arg: copy_spec.destination.filename.clone(),
                    },
                )
            } else {
                // Local source, file: PUT
                (
                    Put::boxed(stream, None, compat),
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
                SetMetadata::boxed(stream, None, compat),
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
