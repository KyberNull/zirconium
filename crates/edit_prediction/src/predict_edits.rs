use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::ops::Range;
use strum::{AsRefStr, EnumString};

/// The name of the header used to indicate which version of Zed the client is running.
pub const ZED_VERSION_HEADER_NAME: &str = "x-zed-version";

/// The name of the header used to indicate which edit prediction experiment should be used.
pub const PREFERRED_EXPERIMENT_HEADER_NAME: &str = "x-zed-preferred-experiment";

/// The name of the header used to indicate the minimum required Zed version.
///
/// This can be used to force a Zed upgrade in order to continue communicating
/// with the LLM service.
pub const MINIMUM_REQUIRED_VERSION_HEADER_NAME: &str = "x-zed-minimum-required-version";

/// The maximum number of edit predictions that can be rejected per request.
pub const MAX_EDIT_PREDICTION_REJECTIONS_PER_REQUEST: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictEditsBody {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub outline: Option<String>,
    pub input_events: String,
    pub input_excerpt: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub speculated_output: Option<String>,
    /// Whether the user provided consent for sampling this interaction.
    #[serde(default, alias = "data_collection_permission")]
    pub can_collect_data: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub diagnostic_groups: Option<Vec<(String, serde_json::Value)>>,
    /// Info about the git repository state, only present when can_collect_data is true.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub git_info: Option<PredictEditsGitInfo>,
    /// The trigger for this request.
    #[serde(default)]
    pub trigger: PredictEditsRequestTrigger,
}

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
    UserInfoChanged,
    VimModeChanged,
    SettingsChanged,
    #[default]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictEditsGitInfo {
    /// SHA of git HEAD commit at time of prediction.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub head_sha: Option<String>,
    /// URL of the remote called `origin`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub remote_origin_url: Option<String>,
    /// URL of the remote called `upstream`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub remote_upstream_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictEditsResponse {
    pub request_id: String,
    pub output_excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptEditPredictionBody {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub e2e_latency_ms: Option<u128>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RejectEditPredictionsBody {
    pub rejections: Vec<EditPredictionRejection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RejectEditPredictionsBodyRef<'a> {
    pub rejections: &'a [EditPredictionRejection],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EditPredictionRejection {
    pub request_id: String,
    #[serde(default)]
    pub reason: EditPredictionRejectReason,
    pub was_shown: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub e2e_latency_ms: Option<u128>,
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

pub const PREDICT_EDITS_MODE_HEADER_NAME: &str = "X-Zed-Predict-Edits-Mode";
pub const PREDICT_EDITS_REQUEST_ID_HEADER_NAME: &str = "X-Zed-Predict-Edits-Request-Id";
pub const PREDICT_EDITS_TRIGGER_HEADER_NAME: &str = "X-Zed-Predict-Edits-Trigger";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, AsRefStr, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PredictEditsMode {
    Eager,
    Subtle,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PredictEditsV3Request {
    #[serde(flatten)]
    pub input: zeta_prompt::Zeta2PromptInput,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PredictEditsV3Response {
    pub request_id: String,
    pub output: String,
    /// The editable region byte range within `cursor_excerpt` that the
    /// server used for this request. When present, the client should use
    /// this range to extract the old text from its local excerpt for
    /// diffing, rather than relying on its own format-derived range.
    pub editable_range: Range<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_version: Option<String>,
    /// Predicted cursor offset within `output`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_offset: Option<usize>,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct PredictEditsV4Request {
    #[serde(flatten)]
    pub input: zeta_prompt::Zeta3PromptInput,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PredictEditsV4Response {
    pub request_id: String,
    pub patch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_version: Option<String>,
}
