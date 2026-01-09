//! Factory functions for creating session command implementations
// (c) 2025 Ross Younger

use crate::protocol::common::{ReceivingStream, SendReceivePair, SendingStream};
use crate::protocol::control::Compatibility;
use crate::protocol::session::{
    Command, CommandParam, Get2Args, GetArgs, ListArgs, Put2Args, PutArgs,
};

use crate::Parameters;
use crate::client::CopyJobSpec;
use crate::session::handler::UI;

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
pub(crate) fn client_sender<'a, S: SendingStream + 'static, R: ReceivingStream + 'static>(
    stream: SendReceivePair<S, R>,
    copy_spec: &CopyJobSpec,
    phase: TransferPhase,
    compat: Compatibility,
    params: &Parameters,
    ui: Option<UI>,
    config: &'a crate::config::Configuration,
) -> (Box<dyn SessionCommandImpl + 'a>, SpanInfo) {
    macro_rules! xreturn {
        ($cmd:expr, $name:expr, $args:expr, $primary_arg:expr) => {
            (
                SessionCommand::boxed(stream, $cmd, $args, compat, config, ui),
                SpanInfo {
                    name: $name,
                    primary_arg: $primary_arg,
                },
            )
        };
    }

    let src = &copy_spec.source.filename;
    let dest = &copy_spec.destination.filename;
    match phase {
        TransferPhase::Pre => {
            // Pre-transfer: list remote directory
            let path = copy_spec.source.filename.clone();
            let mut options = vec![];
            if params.recurse {
                options.push(CommandParam::Recurse.into());
            }
            let args = Some(ListArgs {
                path: path.clone(),
                options,
            });
            xreturn!(ListingHandler, "LIST", args, path)
        }
        TransferPhase::Transfer => {
            if copy_spec.source.user_at_host.is_some() {
                // Remote source: GET
                let mut args = Get2Args::default();
                args.filename.clone_from(&copy_spec.source.filename);
                if copy_spec.preserve {
                    args.options.push(CommandParam::PreserveMetadata.into());
                }
                xreturn!(GetHandler, "GETx", Some(args), src.clone())
            } else if copy_spec.directory {
                // Local source, directory: MKDIR
                xreturn!(CreateDirectoryHandler, "MKDIR", None, dest.clone())
            } else {
                // Local source, file: PUT
                xreturn!(PutHandler, "PUT", None, src.clone())
            }
        }
        TransferPhase::Post => {
            // Post-transfer: set metadata on remote directory
            xreturn!(SetMetadataHandler, "SETMETA", None, dest.clone())
        }
    }
}

/// Factory function to create the appropriate session command handler from a parsed command.
pub(crate) fn command_handler<'a, S: SendingStream + 'static, R: ReceivingStream + 'static>(
    stream: SendReceivePair<S, R>,
    command: Command,
    compat: Compatibility,
    config: &'a crate::config::Configuration,
) -> (Box<dyn SessionCommandImpl + 'a>, SpanInfo) {
    macro_rules! xreturn {
        ($cmd:expr, $name:expr, $args:expr, $primary_arg:expr) => {
            (
                SessionCommand::boxed(stream, $cmd, $args, compat, config, None),
                SpanInfo {
                    name: $name,
                    primary_arg: $primary_arg,
                },
            )
        };
    }

    let (handler, span_info): (Box<dyn SessionCommandImpl>, SpanInfo) = match command {
        Command::Get(GetArgs { filename }) => xreturn!(
            GetHandler,
            "GET",
            Some(Get2Args {
                filename: filename.clone(),
                options: vec![],
            }),
            filename.clone()
        ),
        Command::Get2(args) => {
            let filename = args.filename.clone();
            xreturn!(GetHandler, "GET2", Some(args), filename)
        }
        Command::Put(PutArgs { filename }) => xreturn!(
            PutHandler,
            "PUT",
            Some(Put2Args {
                filename: filename.clone(),
                options: vec![],
            }),
            filename.clone()
        ),
        Command::Put2(args) => {
            let filename = args.filename.clone();
            xreturn!(PutHandler, "PUT2", Some(args), filename)
        }
        Command::CreateDirectory(args) => {
            let dir_name = args.dir_name.clone();
            xreturn!(CreateDirectoryHandler, "MKDIR", Some(args), dir_name)
        }
        Command::SetMetadata(args) => {
            let path = args.path.clone();
            xreturn!(SetMetadataHandler, "SETMETA", Some(args), path)
        }
        Command::List(args) => {
            let path = args.path.clone();
            xreturn!(ListingHandler, "LS", Some(args), path)
        }
    };
    (handler, span_info)
}
