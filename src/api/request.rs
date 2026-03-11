use super::{OutputFormat, RenderConfig};

/// Stable adapter-facing render request envelope.
///
/// This is introduced ahead of the shared runtime facade so CLI, wasm, and
/// direct library entrypoints can converge on one request shape.
#[derive(Debug, Clone, Default)]
pub struct RenderRequest {
    pub input: String,
    pub format: OutputFormat,
    pub config: RenderConfig,
}

impl RenderRequest {
    pub fn new(input: impl Into<String>, format: OutputFormat) -> Self {
        Self {
            input: input.into(),
            format,
            config: RenderConfig::default(),
        }
    }

    pub fn with_config(mut self, config: RenderConfig) -> Self {
        self.config = config;
        self
    }
}
