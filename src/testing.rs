//! Items the isolated test crates use, and nothing else: only `tests/claude.rs`,
//! `tests/codex.rs` and `tests/proxy_env.rs` import from here. They stay separate crates
//! because they set process-wide environment variables. Re-export single items, never a
//! module of the crate, so every other item stays visible to the `dead_code` lint.

pub use crate::llm::call::{call, CallCache, Policy};
pub use crate::llm::Backend;
pub use crate::template::pull::{fetch_catalog, Origin};
pub use crate::tools::{run_capture, RunOpts};

pub mod claude {
    pub use crate::llm::claude::{check, Claude};
}

pub mod codex {
    pub use crate::llm::codex::{check, Codex};
}

pub mod endpoint {
    pub use crate::llm::endpoint::check;
}

pub mod ollama {
    pub use crate::llm::ollama::check;
}

pub mod models {
    pub use crate::models::{pull_with_progress, ModelKind, ModelSpec, Pulled};
}
