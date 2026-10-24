use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Parser;
use minuta::cli::{self, Cli};
use minuta::error::{Error, Result};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let interrupt = Arc::new(AtomicBool::new(false));
    let result = match handle_ctrl_c(&cli, &interrupt) {
        Ok(()) => cli::execute(cli, &interrupt),
        Err(e) => Err(e),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", cli::render_error(&e));
            ExitCode::from(e.exit_code())
        }
    }
}

/// For the pipeline, Ctrl-C sets `interrupt` so the running tool is killed and the job is
/// left clean. Other commands keep the default behaviour of exiting at once.
fn handle_ctrl_c(cli: &Cli, interrupt: &Arc<AtomicBool>) -> Result<()> {
    if !cli.command.runs_pipeline() {
        return Ok(());
    }
    let flag = Arc::clone(interrupt);
    ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst)).map_err(|e| {
        Error::Io(std::io::Error::other(format!(
            "cannot install the Ctrl-C handler: {e}"
        )))
    })
}
