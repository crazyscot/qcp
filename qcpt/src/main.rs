// qcp server - main entrypoint
// (c) 2024 Ross Younger

use clap::Parser as _;
use qcp::server::ServerArgs;

fn main() -> anyhow::Result<()> {
    let args = ServerArgs::parse();
    if args.debug {
        qcp::util::setup_tracing("trace")?;
    }

    qcp::server::main(&args).map_err(|e| {
        tracing::error!("{e}");
        // TODO: Decide about error handling. For now detailed anyhow output is fine.
        e
    })
}
