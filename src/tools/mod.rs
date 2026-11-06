//! External process runner and toolchain preflight. Tools are always run from an argv
//! array, never through a shell.

mod locate;
mod process;
mod progress_line;

pub use locate::{
    inspect, install_hint, locate_tool, preflight, Report, ToolInfo, ToolPaths, Toolchain,
};
pub use process::{run, Output};
pub use process::{run_capture, RunOpts};
pub(crate) use progress_line::{progress_line, ProgressLine};
