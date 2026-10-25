//! The template step: one template run against a job (spec "Template step and job state").

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use crate::error::Result;
use crate::job::Ctx;
use crate::llm::call::Policy;
use crate::llm::chunk::{Event, Generated};
use crate::llm::{Backend, BackendId};
use crate::template::Template;

pub fn output_lang(
    _flag: Option<&str>,
    _template: Option<&str>,
    _config: Option<&str>,
    _detected: &str,
) -> Result<String> {
    todo!()
}

pub fn backend_id(
    _flag: Option<&BackendId>,
    _template: Option<&BackendId>,
    _default: &BackendId,
) -> BackendId {
    todo!()
}

pub struct TemplateRun<'a> {
    pub template: &'a Template,
    pub backend: &'a dyn Backend,
    pub output_lang: String,
    pub forced: bool,
}

impl TemplateRun<'_> {
    pub fn artifact(&self) -> PathBuf {
        todo!()
    }

    pub fn fingerprint(&self, _ctx: &Ctx) -> Result<String> {
        todo!()
    }

    pub fn execute(
        &self,
        _ctx: &Ctx,
        _job_name: &str,
        _policy: &Policy,
        _concurrency: usize,
        _interrupt: &AtomicBool,
        _on_event: &mut dyn FnMut(Event),
    ) -> Result<Generated> {
        todo!()
    }
}
