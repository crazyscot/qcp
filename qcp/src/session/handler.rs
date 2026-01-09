//! Generic command handler wrapper and trait
// (c) 2025 Ross Younger

use anyhow::Result;
use async_trait::async_trait;
use indicatif::{MultiProgress, ProgressBar};

use crate::client::progress::style_for;
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
    async fn send_impl<'a, S: SendingStream, R: ReceivingStream>(
        &mut self,
        inner: &mut SessionCommandInner<'a, S, R>,
        job: &CopyJobSpec,
        params: Parameters,
    ) -> Result<RequestResult>;

    /// Server-side implementation (has access to stream and compat)
    async fn handle_impl<'a, S: SendingStream, R: ReceivingStream>(
        &mut self,
        inner: &mut SessionCommandInner<'a, S, R>,
        args: &Self::Args,
    ) -> Result<()>;
}

/// Client-side UI elements for commands that need them
pub(crate) struct UI {
    display: MultiProgress,
    filename_width: usize,
    spinner: ProgressBar,
}

impl UI {
    pub(crate) fn new(display: MultiProgress, filename_width: usize, spinner: ProgressBar) -> Self {
        Self {
            display,
            filename_width,
            spinner,
        }
    }

    /// Adds a progress bar to the stack (in `self.display`) for the given job.
    pub(crate) fn progress_bar_for(
        &self,
        job: &CopyJobSpec,
        steps: u64,
        quiet: bool,
    ) -> Result<ProgressBar> {
        if quiet {
            return Ok(ProgressBar::hidden());
        }
        let name = format!(
            "{:width$}",
            job.display_filename().to_string_lossy(),
            width = self.filename_width
        );
        Ok(self.display.add(
            ProgressBar::new(steps)
                .with_style(indicatif::ProgressStyle::with_template(style_for(
                    self.filename_width,
                ))?)
                .with_message(name)
                .with_finish(indicatif::ProgressFinish::Abandon),
        ))
    }
}

/// Generic command implementation - the only concrete command type
pub(crate) struct SessionCommand<'a, S: SendingStream, R: ReceivingStream, H: CommandHandler> {
    handler: H,
    args: Option<H::Args>,
    inner: SessionCommandInner<'a, S, R>,
}

/// Inner data for [`SessionCommand`] so it can be borrowed separately from the handler
pub(crate) struct SessionCommandInner<'a, S: SendingStream, R: ReceivingStream> {
    pub stream: SendReceivePair<S, R>,
    /// Negotiated compatibility level
    pub compat: Compatibility,
    /// UI elements for command senders, if desired by the caller.
    ///
    /// Not used by server-side handlers.
    pub ui: UI,
    /// Negotiated configuration
    pub config: &'a Configuration,
}

impl<'a, S: SendingStream, R: ReceivingStream> SessionCommandInner<'a, S, R> {
    fn new_with_ui(
        stream: SendReceivePair<S, R>,
        compat: Compatibility,
        config: &'a Configuration,
        ui: UI,
    ) -> Self {
        Self {
            stream,
            compat,
            ui,
            config,
        }
    }

    fn new_without_ui(
        stream: SendReceivePair<S, R>,
        compat: Compatibility,
        config: &'a Configuration,
    ) -> Self {
        Self {
            stream,
            compat,
            ui: UI {
                display: MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden()),
                filename_width: 0,
                spinner: ProgressBar::hidden(),
            },
            config,
        }
    }

    pub(crate) fn spinner(&self) -> &ProgressBar {
        &self.ui.spinner
    }
}

impl<'a, S: SendingStream + 'static, R: ReceivingStream + 'static, H: CommandHandler + 'static>
    SessionCommand<'a, S, R, H>
{
    /// Create a boxed command for the trait object interface
    pub(crate) fn boxed(
        stream: SendReceivePair<S, R>,
        handler: H,
        args: Option<H::Args>,
        compat: Compatibility,
        config: &'a Configuration,
        ui: Option<UI>,
    ) -> Box<SessionCommand<'a, S, R, H>> {
        if let Some(ui) = ui {
            return Box::new(Self {
                handler,
                args,
                inner: SessionCommandInner::new_with_ui(stream, compat, config, ui),
            });
        }
        Box::new(Self {
            handler,
            args,
            inner: SessionCommandInner::new_without_ui(stream, compat, config),
        })
    }
}

#[async_trait]
impl<S: SendingStream + 'static, R: ReceivingStream + 'static, H: CommandHandler + 'static>
    SessionCommandImpl for SessionCommand<'_, S, R, H>
{
    async fn send(&mut self, job: &CopyJobSpec, params: Parameters) -> Result<RequestResult> {
        self.handler.send_impl(&mut self.inner, job, params).await
    }

    async fn handle(&mut self) -> Result<()> {
        let args = self
            .args
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("command handler missing args"))?;
        self.handler.handle_impl(&mut self.inner, args).await
    }
}

// Re-export handler types for use in factory.rs and tests
pub(crate) use super::{
    get::GetHandler, ls::ListingHandler, mkdir::CreateDirectoryHandler, put::PutHandler,
    set_meta::SetMetadataHandler,
};

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {

    use crate::client::{CopyJobSpec, FileSpec};
    use indicatif::MultiProgress;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_progress_bar_for() {
        let ui = super::UI::new(MultiProgress::new(), 0, indicatif::ProgressBar::hidden());
        let job = CopyJobSpec {
            source: FileSpec {
                filename: "test_file.txt".to_string(),
                ..Default::default()
            },
            destination: FileSpec {
                filename: "dest.txt".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        // Test quiet mode
        let pb = ui.progress_bar_for(&job, 100, true).unwrap();
        assert!(pb.is_hidden());
        assert_eq!(pb.length(), None);
        assert_eq!(pb.message(), "");

        // Test visible mode
        let pb = ui.progress_bar_for(&job, 100, false).unwrap();
        // Checking is_hidden() isn't sound in a CI environment; if stderr isn't to a terminal, is_hidden() always returns true.
        // This can be provoked by rusty_fork_test.
        // But we can still assert about the length and message, which do work as expected (cf. the hidden progress bar above)
        assert_eq!(pb.length(), Some(100));
        assert_eq!(pb.message(), "test_file.txt");
    }
}
