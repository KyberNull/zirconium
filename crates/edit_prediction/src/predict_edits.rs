use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(
    Default, Debug, Clone, Copy, Serialize, Deserialize, strum::AsRefStr, strum::EnumString,
)]
#[strum(serialize_all = "snake_case")]
pub enum PredictEditsRequestTrigger {
    Testing,
    Diagnostics,
    DiagnosticNavigation,
    Cli,
    Explicit,
    BufferEdit,
    LSPCompletionAccepted,
    PredictionAccepted,
    PredictionPartiallyAccepted,
    EditorCreated,
    ProviderChanged,
    VimModeChanged,
    SettingsChanged,
    #[default]
    Other,
}

#[derive(
    Default, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, strum::AsRefStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum EditPredictionRejectReason {
    /// New requests were triggered before this one completed
    Canceled,
    /// No edits returned
    Empty,
    /// Edits returned, but none remained after interpolation
    InterpolatedEmpty,
    /// Edits returned, but could not be interpolated after buffer changes
    InterpolateFailed,
    /// A patch was returned, but could not be applied to the buffer
    PatchApplyFailed,
    /// The new prediction was preferred over the current one
    Replaced,
    /// The current prediction was preferred over the new one
    CurrentPreferred,
    /// The current prediction was discarded
    #[default]
    Discarded,
    /// The current prediction was explicitly rejected by the user
    Rejected,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RawCompletionRequest {
    pub model: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    pub stop: Vec<Cow<'static, str>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RawCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<RawCompletionChoice>,
    pub usage: RawCompletionUsage,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RawCompletionChoice {
    pub text: String,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RawCompletionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
