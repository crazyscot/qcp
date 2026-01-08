//! Generic command handler wrapper and trait
// (c) 2025 Ross Younger

use anyhow::Result;
use async_trait::async_trait;
use indicatif::{MultiProgress, ProgressBar};

use crate::protocol::common::{ReceivingStream, SendReceivePair, SendingStream};
use crate::protocol::control::Compatibility;
use crate::{Parameters, client::CopyJobSpec, config::Configuration};

use super::{RequestResult, SessionCommandImpl};

/// Trait for command-specific behavior - the extension point for new commands
#[async_trait]
pub(crate) trait CommandHandler: Send + Sized {
    /// Associated type for command arguments
    type Args: Send;

    /// Client-side implementation (has access to stream and compat)
    #[allow(clippy::too_many_arguments)] // TODO: refactor later
    async fn send_impl<S: SendingStream, R: ReceivingStream>(
        &mut self,
        stream: &mut SendReceivePair<S, R>,
        compat: Compatibility,
        job: &CopyJobSpec,
        display: MultiProgress,
        filename_width: usize,
        spinner: ProgressBar,
        config: &Configuration,
        params: Parameters,
    ) -> Result<RequestResult>;

    /// Server-side implementation (has access to stream and compat)
    async fn handle_impl<S: SendingStream, R: ReceivingStream>(
        &mut self,
        compat: Compatibility,
        args: &Self::Args,
        io_buffer_size: u64,
        stream: &mut SendReceivePair<S, R>,
    ) -> Result<()>;
}

/// Generic command implementation - the only concrete command type
pub(crate) struct SessionCommand<S: SendingStream, R: ReceivingStream, H: CommandHandler> {
    stream: SendReceivePair<S, R>,
    args: Option<H::Args>,
    compat: crate::protocol::control::Compatibility,
    handler: H,
}

impl<S: SendingStream + 'static, R: ReceivingStream + 'static, H: CommandHandler + 'static>
    SessionCommand<S, R, H>
{
    /// Create a boxed command for the trait object interface
    pub(crate) fn boxed(
        stream: SendReceivePair<S, R>,
        args: Option<H::Args>,
        compat: crate::protocol::control::Compatibility,
        handler: H,
    ) -> Box<dyn SessionCommandImpl> {
        Box::new(Self {
            stream,
            args,
            compat,
            handler,
        })
    }
}

#[async_trait]
impl<S: SendingStream + 'static, R: ReceivingStream + 'static, H: CommandHandler + 'static>
    SessionCommandImpl for SessionCommand<S, R, H>
{
    async fn send(
        &mut self,
        job: &CopyJobSpec,
        display: MultiProgress,
        filename_width: usize,
        spinner: ProgressBar,
        config: &Configuration,
        params: Parameters,
    ) -> Result<RequestResult> {
        self.handler
            .send_impl(
                &mut self.stream,
                self.compat,
                job,
                display,
                filename_width,
                spinner,
                config,
                params,
            )
            .await
    }

    async fn handle(&mut self, io_buffer_size: u64) -> Result<()> {
        let args = self
            .args
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("command handler missing args"))?;
        self.handler
            .handle_impl(self.compat, args, io_buffer_size, &mut self.stream)
            .await
    }
}

// Re-export handler types for use in factory.rs and tests
pub(crate) use super::{
    get::GetHandler, ls::ListingHandler, mkdir::CreateDirectoryHandler, put::PutHandler,
    set_meta::SetMetadataHandler,
};
