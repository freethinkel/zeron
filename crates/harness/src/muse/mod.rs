//! Meta's Muse Code agent, driven natively over the Muse Session Protocol
//! (MSP): `muse serve` speaking JSON-RPC on stdio.

use std::path::PathBuf;

use async_trait::async_trait;
use futures::stream::BoxStream;
use zeron_proto::{AgentEvent, HarnessId, Model, ReasoningLevel, RunRequest};

use crate::{Harness, HarnessError, RunControls, SteeringMode};

/// The Muse Code harness. Construct with [`MuseHarness::new`]; tests point it
/// at a fake CLI with [`MuseHarness::with_executable`].
#[derive(Default)]
pub struct MuseHarness {
    executable: Option<PathBuf>,
}

impl MuseHarness {
    pub fn new() -> Self {
        Self::default()
    }

    /// Use a fixed CLI binary instead of PATH/known-location resolution.
    pub fn with_executable(mut self, path: impl Into<PathBuf>) -> Self {
        self.executable = Some(path.into());
        self
    }

    pub fn resolve_executable(&self) -> Result<PathBuf, HarnessError> {
        if let Some(path) = self
            .executable
            .clone()
            .or_else(|| std::env::var_os("MUSE_EXECUTABLE").map(PathBuf::from))
        {
            return crate::executable::validate_native_override(&path);
        }
        let home = crate::executable::home_or_current_dir();
        crate::executable::find_on_paths(
            "muse",
            vec![
                home.join(".local/bin/muse"),
                PathBuf::from("/opt/homebrew/bin/muse"),
                PathBuf::from("/usr/local/bin/muse"),
            ],
        )
        .ok_or_else(|| {
            HarnessError::NotInstalled(
                "Muse Code CLI: install it (curl -fsSL https://dev.meta.ai/install.sh | sh) or \
                 set MUSE_EXECUTABLE"
                    .into(),
            )
        })
    }
}

#[async_trait]
impl Harness for MuseHarness {
    fn id(&self) -> HarnessId {
        HarnessId::Muse
    }
    fn display_name(&self) -> &str {
        "Muse"
    }
    fn supports_steering(&self) -> bool {
        false
    }
    fn steering_mode(&self) -> SteeringMode {
        SteeringMode::TurnBoundary
    }
    fn reasoning_levels(&self) -> &[ReasoningLevel] {
        &[]
    }
    fn installed(&self) -> bool {
        self.resolve_executable().is_ok()
    }
    fn executable_path(&self) -> Option<PathBuf> {
        self.resolve_executable().ok()
    }
    async fn models(&self) -> Result<Vec<Model>, HarnessError> {
        Ok(Vec::new())
    }
    async fn run(
        &self,
        _request: RunRequest,
        _controls: RunControls,
    ) -> Result<BoxStream<'static, Result<AgentEvent, HarnessError>>, HarnessError> {
        Err(HarnessError::Protocol(
            "Muse sessions are not available yet".into(),
        ))
    }
}
