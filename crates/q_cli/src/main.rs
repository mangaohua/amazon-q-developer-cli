pub mod cli;
pub mod util;

use std::process::ExitCode;

use anstream::eprintln;
use clap::Parser;
use clap::error::{
    ContextKind,
    ErrorKind,
};
use crossterm::style::Stylize;
use eyre::Result;
use fig_log::get_log_level_max;
use fig_util::{
    CLI_BINARY_NAME,
    PRODUCT_NAME,
};
use tracing::metadata::LevelFilter;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> Result<ExitCode> {
    color_eyre::install()?;
    fig_telemetry::set_dispatch_mode(fig_telemetry::DispatchMode::On);
    fig_telemetry::init_global_telemetry_emitter();

    let multithread = matches!(
        std::env::args().nth(1).as_deref(),
        Some("init" | "_" | "internal" | "completion" | "hook")
    );

    let parsed = match cli::Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let _ = err.print();

            let unknown_arg = matches!(err.kind(), ErrorKind::UnknownArgument | ErrorKind::InvalidSubcommand)
                && !err.context().any(|(context_kind, _)| {
                    matches!(
                        context_kind,
                        ContextKind::SuggestedSubcommand | ContextKind::SuggestedArg
                    )
                });

            if unknown_arg {
                eprintln!(
                    "\nThis command may be valid in newer versions of the {PRODUCT_NAME} CLI. Try running {} {}.",
                    CLI_BINARY_NAME.magenta(),
                    "update".magenta()
                );
            }

            return Ok(ExitCode::from(err.exit_code().try_into().unwrap_or(2)));
        },
    };

    let verbose = parsed.verbose > 0;

    let runtime = if multithread {
        tokio::runtime::Builder::new_multi_thread()
    } else {
        tokio::runtime::Builder::new_current_thread()
    }
    .enable_all()
    .build()?;

    let result = runtime.block_on(async {
        let result = parsed.execute().await;
        fig_telemetry::finish_telemetry().await;
        result
    });

    match result {
        Ok(exit_code) => Ok(exit_code),
        Err(err) => {
            if verbose || get_log_level_max() > LevelFilter::INFO {
                eprintln!("{} {err:?}", "error:".bold().red());
            } else {
                eprintln!("{} {err}", "error:".bold().red());
            }
            Ok(ExitCode::FAILURE)
        },
    }
}
