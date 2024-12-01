//! Main CLI entrypoint for qcp
// (c) 2024 Ross Younger

use std::process::ExitCode;

use super::{args::CliArgs, styles};

use crate::{
    client::{client_main, Behaviours, MAX_UPDATE_FPS},
    config::{Configuration, Manager},
    os,
    server::server_main,
    transport::BandwidthParams,
    util::setup_tracing,
};
use indicatif::{MultiProgress, ProgressDrawTarget};
use tracing::error_span;

fn trace_level(args: &Behaviours) -> &str {
    if args.debug {
        "debug"
    } else if args.quiet {
        "error"
    } else {
        "info"
    }
}

/// Main CLI entrypoint
///
/// Call this from `main`. It reads argv.
/// # Exit status
/// 0 indicates success; non-zero indicates failure.
#[tokio::main(flavor = "current_thread")]
#[allow(clippy::missing_panics_doc)]
pub async fn cli() -> anyhow::Result<ExitCode> {
    let args = CliArgs::custom_parse();
    if args.help_buffers {
        os::print_udp_buffer_size_help_message(
            BandwidthParams::recv_buffer(),
            BandwidthParams::send_buffer(),
        );
        return Ok(ExitCode::SUCCESS);
    }

    let progress = (!args.server).then(|| {
        MultiProgress::with_draw_target(ProgressDrawTarget::stderr_with_hz(MAX_UPDATE_FPS))
    });
    setup_tracing(
        trace_level(&args.behaviours),
        progress.as_ref(),
        &args.behaviours.log_file,
    )
    .inspect_err(|e| eprintln!("{e:?}"))?;

    if args.config_files {
        // do this before attempting to read config, in case it fails
        println!("{:?}", Manager::config_files());
        return Ok(ExitCode::SUCCESS);
    }

    // Now fold the arguments in with the CLI config (which may fail)
    let config_manager = Manager::from(args.clone()); // TODO declone

    let config = match config_manager.get::<Configuration>() {
        Ok(c) => c,
        Err(err) => {
            println!(
                "{}: Failed to parse configuration",
                styles::error().apply_to("ERROR")
            );
            if err.count() == 1 {
                println!("{err}");
            } else {
                let inf = styles::info();
                for (i, e) in err.into_iter().enumerate() {
                    println!("{}: {e}", inf.apply_to(i + 1));
                }
            }
            return Ok(ExitCode::FAILURE);
        }
    };

    if args.show_config {
        println!("{config_manager}");
        Ok(ExitCode::SUCCESS)
    } else if args.server {
        let _span = error_span!("REMOTE").entered();
        server_main(config.bandwidth, config.quic)
            .await
            .map(|()| ExitCode::SUCCESS)
            .inspect_err(|e| tracing::error!("{e}"))
    } else {
        let job_spec = crate::client::CopyJobSpec::try_from(&args)?;
        client_main(
            config.client,
            config.bandwidth,
            config.quic,
            progress.unwrap(),
            job_spec,
            args.behaviours,
        )
        .await
        .inspect_err(|e| tracing::error!("{e}"))
        .or_else(|_| Ok(false))
        .map(|success| {
            if success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        })
    }
}
