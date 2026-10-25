use crate::predict_edits::{EditPredictionRejectReason, PredictEditsRequestTrigger};
use anyhow::{Context as _, Result};
use collections::{HashMap, HashSet};
use copilot::{Copilot, Reinstall};
use credentials_provider::CredentialsProvider;
use edit_prediction_context::RelatedFile;
use edit_prediction_types::EditPredictionRequestTrigger;
use futures::channel::mpsc;
use git::repository::FileHistoryChangedFileSets;
use gpui::{
    App, AsyncApp, Context, Entity, EntityId, Global, Task, WeakEntity, actions, prelude::*,
};
use heapless::Vec as ArrayVec;
use language::{
    Anchor, Buffer, BufferEditSource, BufferSnapshot, EditPredictionPromptFormat, File,
    OffsetRangeExt, Point, TextBufferSnapshot, ToOffset, ToPoint,
    language_settings::all_language_settings,
};
use project::{DisableAiSettings, Project, ProjectPath, WorktreeId};
use settings::{EditPredictionProvider, Settings as _};
use std::collections::{VecDeque, hash_map};
use std::rc::Rc;
use text::{AnchorRangeExt, Edit};
use workspace::{AppState, Workspace};
use zeta_prompt::ContextSource;

use std::mem;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use util::{ResultExt as _, rel_path::RelPath};

pub mod cursor_excerpt;
pub mod data_collection;
pub mod example_spec;
pub mod fim;
mod license_detection;
pub mod mercury;
pub mod metrics;
pub mod ollama;
pub mod open_ai_response;
pub mod predict_edits;
mod prediction;
pub mod sweep_prompt;

pub mod udiff;

pub mod open_ai_compatible;
mod zed_edit_prediction_delegate;
pub mod zeta;

#[cfg(test)]
mod edit_prediction_tests;

use crate::cursor_excerpt::expand_context_syntactically_then_linewise;
use crate::example_spec::RecentFile;
use crate::license_detection::LicenseDetectionWatcher;
use crate::mercury::Mercury;
pub use crate::metrics::{KeptRateResult, compute_kept_rate};
use crate::prediction::EditPredictionResult;
pub use crate::prediction::{EditPrediction, EditPredictionId, EditPredictionInputs};
pub use language_model::ApiKeyState;
pub use telemetry_events::EditPredictionRating;
pub use zed_edit_prediction_delegate::ZedEditPredictionDelegate;

actions!(
    edit_prediction,
    [
        /// Clears the edit prediction history.
        ClearHistory,
    ]
);

/// Maximum number of events to track.
const EVENT_COUNT_MAX: usize = 10;
const RECENT_PATH_COUNT_MAX: usize = 20;
const CHANGE_GROUPING_LINE_SPAN: u32 = 8;
const EDIT_HISTORY_DIFF_SIZE_LIMIT: usize = 2048 * 3; // ~2048 tokens or ~50% of typical prompt budget
const COLLABORATOR_EDIT_LOCALITY_CONTEXT_TOKENS: usize = 512;
const GIT_CHANGED_FILE_SETS_COMMIT_LIMIT: usize = 100;
const LAST_CHANGE_GROUPING_TIME: Duration = Duration::from_secs(1);

#[derive(Clone)]
struct EditPredictionStoreGlobal(Entity<EditPredictionStore>);

impl Global for EditPredictionStoreGlobal {}

pub struct EditPredictionStore {
    projects: HashMap<EntityId, ProjectState>,
    edit_prediction_model: EditPredictionModel,
    pub mercury: Mercury,
    credentials_provider: Arc<dyn CredentialsProvider>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum EditPredictionModel {
    Zeta,
    Fim { format: EditPredictionPromptFormat },
    SweepPrompt,
    Mercury,
}

pub struct EditPredictionModelInput {
    buffer: Entity<Buffer>,
    snapshot: BufferSnapshot,
    position: Anchor,
    events: Vec<Arc<zeta_prompt::Event>>,
    stored_events: Vec<StoredEvent>,
    related_files: Vec<RelatedFile>,
    trigger: PredictEditsRequestTrigger,
    diagnostic_search_range: Range<Point>,
    debug_tx: Option<mpsc::UnboundedSender<DebugEvent>>,
    is_open_source: bool,
}

#[derive(Debug)]
pub enum DebugEvent {
    EditPredictionStarted(EditPredictionStartedDebugEvent),
    EditPredictionFinished(EditPredictionFinishedDebugEvent),
}

#[derive(Debug)]
pub struct EditPredictionStartedDebugEvent {
    pub buffer: WeakEntity<Buffer>,
    pub position: Anchor,
    pub prompt: Option<String>,
}

#[derive(Debug)]
pub struct EditPredictionFinishedDebugEvent {
    pub buffer: WeakEntity<Buffer>,
    pub position: Anchor,
    pub model_output: Option<String>,
}

/// An event with associated metadata for reconstructing buffer state.
#[derive(Clone)]
pub struct StoredEvent {
    pub event: Arc<zeta_prompt::Event>,
    pub old_snapshot: TextBufferSnapshot,
    pub new_snapshot_version: clock::Global,
    pub total_edit_range: Range<Anchor>,
    pub(crate) file_context: Option<Entity<StoredFileContext>>,
}

pub(crate) struct StoredFileContext {
    pub(crate) git_changed_file_sets: Option<Arc<FileHistoryChangedFileSets>>,
    pub(crate) git_changed_file_sets_task: Option<Task<()>>,
}

impl StoredEvent {
    fn can_merge(
        &self,
        next_old_event: &StoredEvent,
        latest_snapshot: &TextBufferSnapshot,
        latest_edit_range: &Range<Anchor>,
    ) -> bool {
        // Events must be for the same buffer and be contiguous across included snapshots to be mergeable.
        if self.old_snapshot.remote_id() != next_old_event.old_snapshot.remote_id() {
            return false;
        }
        if self.old_snapshot.remote_id() != latest_snapshot.remote_id() {
            return false;
        }
        if self.new_snapshot_version != next_old_event.old_snapshot.version {
            return false;
        }
        if !latest_snapshot
            .version
            .observed_all(&next_old_event.new_snapshot_version)
        {
            return false;
        }

        let a_is_predicted = matches!(
            self.event.as_ref(),
            zeta_prompt::Event::BufferChange {
                predicted: true,
                ..
            }
        );
        let b_is_predicted = matches!(
            next_old_event.event.as_ref(),
            zeta_prompt::Event::BufferChange {
                predicted: true,
                ..
            }
        );

        // If events come from the same source (both predicted or both manual) then
        // we would have coalesced them already.
        if a_is_predicted == b_is_predicted {
            return false;
        }

        let left_range = self.total_edit_range.to_point(latest_snapshot);
        let right_range = next_old_event.total_edit_range.to_point(latest_snapshot);
        let latest_range = latest_edit_range.to_point(latest_snapshot);

        // Events near to the latest edit are not merged if their sources differ.
        if lines_between_ranges(&left_range, &latest_range)
            .min(lines_between_ranges(&right_range, &latest_range))
            <= CHANGE_GROUPING_LINE_SPAN
        {
            return false;
        }

        // Events that are distant from each other are not merged.
        if lines_between_ranges(&left_range, &right_range) > CHANGE_GROUPING_LINE_SPAN {
            return false;
        }

        true
    }
}

fn lines_between_ranges(left: &Range<Point>, right: &Range<Point>) -> u32 {
    if left.start > right.end {
        return left.start.row - right.end.row;
    }
    if right.start > left.end {
        return right.start.row - left.end.row;
    }
    0
}

fn push_recent_file(files: &mut VecDeque<RecentFile>, mut file: RecentFile) {
    if let Some(ix) = files.iter().position(|probe| probe.path == file.path)
        && let Some(previous) = files.remove(ix)
        && file.cursor_position.is_none()
    {
        file.cursor_position = previous.cursor_position;
    }
    files.push_front(file);
    files.truncate(RECENT_PATH_COUNT_MAX);
}

struct ProjectState {
    events: VecDeque<StoredEvent>,
    last_event: Option<LastEvent>,
    recently_viewed_files: VecDeque<RecentFile>,
    recently_opened_files: VecDeque<RecentFile>,
    registered_buffers: HashMap<gpui::EntityId, RegisteredBuffer>,
    file_contexts: HashMap<ProjectPath, WeakEntity<StoredFileContext>>,
    current_prediction: Option<CurrentEditPrediction>,
    last_edit_source: Option<BufferEditSource>,
    next_pending_prediction_id: usize,
    pending_predictions: ArrayVec<PendingPrediction, 2, u8>,
    debug_tx: Option<mpsc::UnboundedSender<DebugEvent>>,
    last_edit_prediction_refresh: Option<(EntityId, Instant)>,
    cancelled_predictions: HashSet<usize>,
    license_detection_watchers: HashMap<WorktreeId, Rc<LicenseDetectionWatcher>>,
    _subscriptions: [gpui::Subscription; 2],
    copilot: Option<Entity<Copilot>>,
}

impl ProjectState {
    pub fn events(&self, cx: &App) -> Vec<StoredEvent> {
        self.events
            .iter()
            .cloned()
            .chain(self.last_event.as_ref().iter().flat_map(|event| {
                let (one, two) = event.split_by_pause();
                let one = one.finalize(&self.license_detection_watchers, cx);
                let two = two.and_then(|two| two.finalize(&self.license_detection_watchers, cx));
                one.into_iter().chain(two)
            }))
            .collect()
    }

    fn cancel_pending_prediction(
        &mut self,
        pending_prediction: PendingPrediction,
        cx: &mut Context<EditPredictionStore>,
    ) {
        self.cancelled_predictions.insert(pending_prediction.id);

        if pending_prediction.drop_on_cancel {
            drop(pending_prediction.task);
        } else {
            cx.spawn(async move |this, cx| {
                let Some(prediction_id) = pending_prediction.task.await else {
                    return;
                };

                this.update(cx, |this, cx| {
                    this.reject_prediction(
                        prediction_id,
                        EditPredictionRejectReason::Canceled,
                        false,
                        cx,
                    );
                })
                .ok();
            })
            .detach()
        }
    }

    fn active_buffer(
        &self,
        project: &Entity<Project>,
        cx: &App,
    ) -> Option<(Entity<Buffer>, Option<Anchor>)> {
        let project = project.read(cx);
        let active_path = project.path_for_entry(project.active_entry()?, cx)?;
        let active_buffer = project.buffer_store().read(cx).get_by_path(&active_path)?;
        let registered_buffer = self.registered_buffers.get(&active_buffer.entity_id())?;
        Some((active_buffer, registered_buffer.last_position))
    }

    fn file_context_for_path(
        &mut self,
        path: ProjectPath,
        cx: &mut Context<EditPredictionStore>,
    ) -> Entity<StoredFileContext> {
        if let Some(context) = self
            .file_contexts
            .get_mut(&path)
            .and_then(|entry| entry.upgrade())
        {
            context
        } else {
            let context = cx.new(|_| StoredFileContext {
                git_changed_file_sets: None,
                git_changed_file_sets_task: None,
            });
            self.file_contexts.insert(path, context.downgrade());
            context
        }
    }

    fn update_recent_file_cursor(&mut self, path: &Path, cursor_position: usize) {
        for file in &mut self.recently_opened_files {
            if file.path.as_ref() == path && file.cursor_position.is_none() {
                file.cursor_position = Some(cursor_position);
            }
        }
        for file in &mut self.recently_viewed_files {
            if file.path.as_ref() == path {
                file.cursor_position = Some(cursor_position);
            }
        }
    }

    fn finalize_last_event(&mut self, cx: &mut Context<EditPredictionStore>) {
        let Some(last_event) = self.last_event.take() else {
            return;
        };
        let event = last_event.finalize(&self.license_detection_watchers, cx);

        let Some(event) = event else {
            return;
        };
        if self.events.len() + 1 >= EVENT_COUNT_MAX {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    fn clear_history(&mut self) {
        self.events.clear();
        self.last_event.take();
    }
}

#[derive(Debug, Clone)]
struct CurrentEditPrediction {
    pub requested_by: EntityId,
    pub prediction: EditPrediction,
    pub was_shown: bool,
    pub shown_with: Option<edit_prediction_types::SuggestionDisplayType>,
}

impl CurrentEditPrediction {
    fn should_replace_prediction(&self, old_prediction: &Self, cx: &App) -> bool {
        let Some(new_edits) = self
            .prediction
            .interpolate(&self.prediction.buffer.read(cx))
        else {
            return false;
        };

        if self.prediction.buffer != old_prediction.prediction.buffer {
            return true;
        }

        let Some(old_edits) = old_prediction
            .prediction
            .interpolate(&old_prediction.prediction.buffer.read(cx))
        else {
            return true;
        };

        // This reduces the occurrence of UI thrash from replacing edits
        //
        // TODO: This is fairly arbitrary - should have a more general heuristic that handles multiple edits.
        if self.requested_by == self.prediction.buffer.entity_id()
            && self.requested_by == old_prediction.prediction.buffer.entity_id()
            && old_edits.len() == 1
            && new_edits.len() == 1
        {
            let (old_range, old_text) = &old_edits[0];
            let (new_range, new_text) = &new_edits[0];
            new_range == old_range && new_text.starts_with(old_text.as_ref())
        } else {
            true
        }
    }
}

const DIAGNOSTIC_LINES_RANGE: u32 = 20;

#[derive(Debug)]
struct PendingPrediction {
    id: usize,
    task: Task<Option<EditPredictionId>>,
    /// If true, the task is dropped immediately on cancel (cancelling the HTTP request).
    /// If false, the task is awaited to completion so rejection can be reported.
    drop_on_cancel: bool,
}

/// A prediction from the perspective of a buffer.
#[derive(Debug)]
enum BufferEditPrediction<'a> {
    Local { prediction: &'a EditPrediction },
    Jump { prediction: &'a EditPrediction },
}

#[cfg(test)]
impl std::ops::Deref for BufferEditPrediction<'_> {
    type Target = EditPrediction;

    fn deref(&self) -> &Self::Target {
        match self {
            BufferEditPrediction::Local { prediction } => prediction,
            BufferEditPrediction::Jump { prediction } => prediction,
        }
    }
}

struct RegisteredBuffer {
    file: Option<Arc<dyn File>>,
    snapshot: TextBufferSnapshot,
    last_position: Option<Anchor>,
    _subscriptions: [gpui::Subscription; 2],
}

#[derive(Clone)]
struct LastEvent {
    old_snapshot: TextBufferSnapshot,
    new_snapshot: TextBufferSnapshot,
    old_file: Option<Arc<dyn File>>,
    new_file: Option<Arc<dyn File>>,
    latest_edit_range: Range<Anchor>,
    total_edit_range: Range<Anchor>,
    total_edit_range_at_last_pause_boundary: Option<Range<Anchor>>,
    predicted: bool,
    snapshot_after_last_editing_pause: Option<TextBufferSnapshot>,
    last_edit_time: Option<Instant>,
    file_context: Option<Entity<StoredFileContext>>,
}

impl LastEvent {
    pub fn finalize(
        &self,
        license_detection_watchers: &HashMap<WorktreeId, Rc<LicenseDetectionWatcher>>,
        cx: &App,
    ) -> Option<StoredEvent> {
        let path = buffer_path_with_id_fallback(self.new_file.as_ref(), &self.new_snapshot, cx);
        let old_path = buffer_path_with_id_fallback(self.old_file.as_ref(), &self.old_snapshot, cx);

        let in_open_source_repo =
            [self.new_file.as_ref(), self.old_file.as_ref()]
                .iter()
                .all(|file| {
                    file.is_some_and(|file| {
                        license_detection_watchers
                            .get(&file.worktree_id(cx))
                            .is_some_and(|watcher| watcher.is_project_open_source())
                    })
                });

        let (diff, old_range, new_range) = compute_diff_between_snapshots_in_range(
            &self.old_snapshot,
            &self.new_snapshot,
            &self.total_edit_range,
        )?;

        if path == old_path && diff.is_empty() {
            None
        } else {
            Some(StoredEvent {
                event: Arc::new(zeta_prompt::Event::BufferChange {
                    old_path,
                    path,
                    diff,
                    old_range,
                    new_range: new_range.clone(),
                    in_open_source_repo,
                    predicted: self.predicted,
                }),
                old_snapshot: self.old_snapshot.clone(),
                new_snapshot_version: self.new_snapshot.version.clone(),
                total_edit_range: self.new_snapshot.anchor_before(new_range.start)
                    ..self.new_snapshot.anchor_before(new_range.end),
                file_context: self.file_context.clone(),
            })
        }
    }

    pub fn split_by_pause(&self) -> (LastEvent, Option<LastEvent>) {
        let Some(boundary_snapshot) = self.snapshot_after_last_editing_pause.as_ref() else {
            return (self.clone(), None);
        };

        let Some(after) = self.suffix_after(boundary_snapshot) else {
            return (self.clone(), None);
        };

        let total_edit_range_before_pause = self
            .total_edit_range_at_last_pause_boundary
            .clone()
            .unwrap_or_else(|| self.total_edit_range.clone());

        let before = LastEvent {
            new_snapshot: boundary_snapshot.clone(),
            latest_edit_range: total_edit_range_before_pause.clone(),
            total_edit_range: total_edit_range_before_pause,
            total_edit_range_at_last_pause_boundary: None,
            snapshot_after_last_editing_pause: None,
            ..self.clone()
        };

        (before, Some(after))
    }

    /// The portion of this event that happened after `boundary_snapshot`, or
    /// None if the buffer hasn't changed since.
    pub fn suffix_after(&self, boundary_snapshot: &TextBufferSnapshot) -> Option<LastEvent> {
        let total_edit_range =
            compute_total_edit_range_between_snapshots(boundary_snapshot, &self.new_snapshot)?;
        Some(LastEvent {
            old_snapshot: boundary_snapshot.clone(),
            latest_edit_range: total_edit_range.clone(),
            total_edit_range,
            total_edit_range_at_last_pause_boundary: None,
            snapshot_after_last_editing_pause: None,
            ..self.clone()
        })
    }
}

fn compute_total_edit_range_between_snapshots(
    old_snapshot: &TextBufferSnapshot,
    new_snapshot: &TextBufferSnapshot,
) -> Option<Range<Anchor>> {
    let edits: Vec<Edit<usize>> = new_snapshot
        .edits_since::<usize>(&old_snapshot.version)
        .collect();

    let (first_edit, last_edit) = edits.first().zip(edits.last())?;
    let new_start_point = new_snapshot.offset_to_point(first_edit.new.start);
    let new_end_point = new_snapshot.offset_to_point(last_edit.new.end);

    Some(new_snapshot.anchor_before(new_start_point)..new_snapshot.anchor_before(new_end_point))
}

fn compute_old_range_for_new_range(
    old_snapshot: &TextBufferSnapshot,
    new_snapshot: &TextBufferSnapshot,
    total_edit_range: &Range<Anchor>,
) -> Option<Range<Point>> {
    let new_start_offset = total_edit_range.start.to_offset(new_snapshot);
    let new_end_offset = total_edit_range.end.to_offset(new_snapshot);

    let edits: Vec<Edit<usize>> = new_snapshot
        .edits_since::<usize>(&old_snapshot.version)
        .collect();
    let mut old_start_offset = None;
    let mut old_end_offset = None;
    let mut delta: isize = 0;

    for edit in &edits {
        if old_start_offset.is_none() && new_start_offset <= edit.new.end {
            old_start_offset = Some(if new_start_offset < edit.new.start {
                new_start_offset.checked_add_signed(-delta)?
            } else {
                edit.old.start
            });
        }

        if old_end_offset.is_none() && new_end_offset <= edit.new.end {
            old_end_offset = Some(if new_end_offset < edit.new.start {
                new_end_offset.checked_add_signed(-delta)?
            } else {
                edit.old.end
            });
        }

        delta += edit.new.len() as isize - edit.old.len() as isize;
    }

    let old_start_offset =
        old_start_offset.unwrap_or_else(|| new_start_offset.saturating_add_signed(-delta));
    let old_end_offset =
        old_end_offset.unwrap_or_else(|| new_end_offset.saturating_add_signed(-delta));

    Some(
        old_snapshot.offset_to_point(old_start_offset)
            ..old_snapshot.offset_to_point(old_end_offset),
    )
}

fn compute_diff_between_snapshots_in_range(
    old_snapshot: &TextBufferSnapshot,
    new_snapshot: &TextBufferSnapshot,
    total_edit_range: &Range<Anchor>,
) -> Option<(String, Range<usize>, Range<usize>)> {
    let new_start_offset = total_edit_range.start.to_offset(new_snapshot);
    let new_end_offset = total_edit_range.end.to_offset(new_snapshot);
    let new_start_point = new_snapshot.offset_to_point(new_start_offset);
    let new_end_point = new_snapshot.offset_to_point(new_end_offset);
    let old_range = compute_old_range_for_new_range(old_snapshot, new_snapshot, total_edit_range)?;
    let old_start_point = old_range.start;
    let old_end_point = old_range.end;
    let old_start_offset = old_snapshot.point_to_offset(old_start_point);
    let old_end_offset = old_snapshot.point_to_offset(old_end_point);

    const CONTEXT_LINES: u32 = 3;

    let old_context_start_row = old_start_point.row.saturating_sub(CONTEXT_LINES);
    let new_context_start_row = new_start_point.row.saturating_sub(CONTEXT_LINES);
    let old_context_end_row =
        (old_end_point.row + 1 + CONTEXT_LINES).min(old_snapshot.max_point().row);
    let new_context_end_row =
        (new_end_point.row + 1 + CONTEXT_LINES).min(new_snapshot.max_point().row);

    let old_start_line_offset = old_snapshot.point_to_offset(Point::new(old_context_start_row, 0));
    let new_start_line_offset = new_snapshot.point_to_offset(Point::new(new_context_start_row, 0));
    let old_end_line_offset = old_snapshot
        .point_to_offset(Point::new(old_context_end_row + 1, 0).min(old_snapshot.max_point()));
    let new_end_line_offset = new_snapshot
        .point_to_offset(Point::new(new_context_end_row + 1, 0).min(new_snapshot.max_point()));
    let old_edit_range = old_start_line_offset..old_end_line_offset;
    let new_edit_range = new_start_line_offset..new_end_line_offset;

    if new_edit_range.len() > EDIT_HISTORY_DIFF_SIZE_LIMIT
        || old_edit_range.len() > EDIT_HISTORY_DIFF_SIZE_LIMIT
    {
        return None;
    }

    let old_region_text: String = old_snapshot.text_for_range(old_edit_range).collect();
    let new_region_text: String = new_snapshot.text_for_range(new_edit_range).collect();

    let diff = language::unified_diff_with_offsets(
        &old_region_text,
        &new_region_text,
        old_context_start_row,
        new_context_start_row,
    );

    Some((
        diff,
        old_start_offset..old_end_offset,
        new_start_offset..new_end_offset,
    ))
}

pub(crate) fn buffer_path_with_id_fallback(
    file: Option<&Arc<dyn File>>,
    snapshot: &TextBufferSnapshot,
    cx: &App,
) -> Arc<Path> {
    let Some(file) = file else {
        return Path::new(&format!("untitled-{}", snapshot.remote_id())).into();
    };
    let full_path = file.full_path(cx);
    let Some(path) = RelPath::new(&full_path, file.path_style(cx)).ok() else {
        return Path::new(&format!("untitled-{}", snapshot.remote_id())).into();
    };
    path.as_std_path().into()
}

fn predict_edits_request_trigger_from_editor_trigger(
    trigger: EditPredictionRequestTrigger,
) -> PredictEditsRequestTrigger {
    match trigger {
        EditPredictionRequestTrigger::DiagnosticNavigation => {
            PredictEditsRequestTrigger::DiagnosticNavigation
        }
        EditPredictionRequestTrigger::Explicit => PredictEditsRequestTrigger::Explicit,
        EditPredictionRequestTrigger::BufferEdit => PredictEditsRequestTrigger::BufferEdit,
        EditPredictionRequestTrigger::LSPCompletionAccepted => {
            PredictEditsRequestTrigger::LSPCompletionAccepted
        }
        EditPredictionRequestTrigger::PredictionAccepted => {
            PredictEditsRequestTrigger::PredictionAccepted
        }
        EditPredictionRequestTrigger::PredictionPartiallyAccepted => {
            PredictEditsRequestTrigger::PredictionPartiallyAccepted
        }
        EditPredictionRequestTrigger::EditorCreated => PredictEditsRequestTrigger::EditorCreated,
        EditPredictionRequestTrigger::ProviderChanged => {
            PredictEditsRequestTrigger::ProviderChanged
        }
        EditPredictionRequestTrigger::VimModeChanged => PredictEditsRequestTrigger::VimModeChanged,
        EditPredictionRequestTrigger::SettingsChanged => {
            PredictEditsRequestTrigger::SettingsChanged
        }
        EditPredictionRequestTrigger::Other => PredictEditsRequestTrigger::Other,
    }
}

impl EditPredictionStore {
    pub fn try_global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<EditPredictionStoreGlobal>()
            .map(|global| global.0.clone())
    }

    pub fn global(cx: &mut App) -> Entity<Self> {
        cx.try_global::<EditPredictionStoreGlobal>()
            .map(|global| global.0.clone())
            .unwrap_or_else(|| {
                let ep_store = cx.new(Self::new);
                cx.set_global(EditPredictionStoreGlobal(ep_store.clone()));
                ep_store
            })
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        let credentials_provider = zed_credentials_provider::global(cx);

        Self {
            projects: HashMap::default(),
            edit_prediction_model: EditPredictionModel::Zeta,
            mercury: Mercury::new(cx),
            credentials_provider,
        }
    }

    pub fn set_edit_prediction_model(&mut self, model: EditPredictionModel) {
        self.edit_prediction_model = model;
    }

    pub fn icons(&self, cx: &App) -> edit_prediction_types::EditPredictionIconSet {
        use ui::IconName;
        match self.edit_prediction_model {
            EditPredictionModel::Mercury => {
                edit_prediction_types::EditPredictionIconSet::new(IconName::Inception)
            }
            EditPredictionModel::Zeta => {
                edit_prediction_types::EditPredictionIconSet::new(IconName::ZedPredict)
                    .with_disabled(IconName::ZedPredictDisabled)
                    .with_up(IconName::ZedPredictUp)
                    .with_down(IconName::ZedPredictDown)
                    .with_error(IconName::ZedPredictError)
            }
            EditPredictionModel::Fim { .. } | EditPredictionModel::SweepPrompt => {
                let settings = &all_language_settings(None, cx).edit_predictions;
                match settings.provider {
                    EditPredictionProvider::Ollama => {
                        edit_prediction_types::EditPredictionIconSet::new(IconName::AiOllama)
                    }
                    _ => {
                        edit_prediction_types::EditPredictionIconSet::new(IconName::AiOpenAiCompat)
                    }
                }
            }
        }
    }

    pub fn has_mercury_api_token(&self, cx: &App) -> bool {
        self.mercury.api_token.read(cx).has_key()
    }

    pub fn mercury_has_payment_required_error(&self) -> bool {
        self.mercury.has_payment_required_error()
    }

    pub fn clear_history(&mut self) {
        for project_state in self.projects.values_mut() {
            project_state.clear_history();
        }
    }

    pub fn clear_history_for_project(&mut self, project: &Entity<Project>) {
        if let Some(project_state) = self.projects.get_mut(&project.entity_id()) {
            project_state.clear_history();
        }
    }

    pub fn edit_history_for_project(
        &self,
        project: &Entity<Project>,
        cx: &App,
    ) -> Vec<StoredEvent> {
        self.projects
            .get(&project.entity_id())
            .map(|project_state| project_state.events(cx))
            .unwrap_or_default()
    }

    pub fn copilot_for_project(&self, project: &Entity<Project>) -> Option<Entity<Copilot>> {
        self.projects
            .get(&project.entity_id())
            .and_then(|project| project.copilot.clone())
    }

    pub fn start_copilot_for_project(
        &mut self,
        project: &Entity<Project>,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Copilot>> {
        if DisableAiSettings::get(None, cx).disable_ai {
            return None;
        }
        let state = self.get_or_init_project(project, cx);

        if state.copilot.is_some() {
            return state.copilot.clone();
        }
        let _project = project.clone();
        let project = project.read(cx);

        let node = project.node_runtime().cloned();
        if let Some(node) = node {
            let next_id = project.languages().next_language_server_id();
            let fs = project.fs().clone();

            let copilot = cx.new(|cx| Copilot::new(Some(_project), next_id, fs, node, cx));
            state.copilot = Some(copilot.clone());
            Some(copilot)
        } else {
            None
        }
    }

    pub fn register_project(&mut self, project: &Entity<Project>, cx: &mut Context<Self>) {
        self.get_or_init_project(project, cx);
    }

    pub fn register_buffer(
        &mut self,
        buffer: &Entity<Buffer>,
        project: &Entity<Project>,
        cx: &mut Context<Self>,
    ) {
        let opened_path = buffer
            .read(cx)
            .file()
            .map(|file| ProjectPath::from_file(file.as_ref(), cx));
        let project_state = self.get_or_init_project(project, cx);
        if let Some(path) = opened_path {
            push_recent_file(
                &mut project_state.recently_opened_files,
                RecentFile {
                    path: path.path.as_std_path().into(),
                    cursor_position: None,
                },
            );
        }
        Self::register_buffer_impl(project_state, buffer, project, cx);
    }

    fn ensure_git_changed_file_sets_loading(
        file_context: &Entity<StoredFileContext>,
        project: &Entity<Project>,
        project_path: &ProjectPath,
        cx: &mut Context<Self>,
    ) {
        let should_start = file_context.update(cx, |file_context, _| {
            file_context.git_changed_file_sets.is_none()
                && file_context.git_changed_file_sets_task.is_none()
        });
        if !should_start {
            return;
        }

        let Some((repository, repo_path)) = project
            .read(cx)
            .git_store()
            .read(cx)
            .repository_and_path_for_project_path(project_path, cx)
        else {
            file_context.update(cx, |file_context, _| {
                file_context.git_changed_file_sets = Some(Arc::default());
            });
            return;
        };

        let receiver = repository.update(cx, |repository, _| {
            repository
                .file_history_changed_files(vec![repo_path], GIT_CHANGED_FILE_SETS_COMMIT_LIMIT)
        });
        let task = cx.spawn({
            let file_context = file_context.downgrade();
            async move |_, cx| {
                let result = receiver.await;
                let Some(file_context) = file_context.upgrade() else {
                    return;
                };
                file_context.update(cx, |file_context, _| {
                    file_context.git_changed_file_sets = result
                        .context("failed to receive git changed file sets")
                        .flatten()
                        .log_with_level(log::Level::Trace)
                        .map(|mut file_sets| file_sets.pop().unwrap_or_default())
                        .context("failed to load git changed file sets")
                        .map(Arc::new)
                        .log_err();
                    file_context.git_changed_file_sets_task = None;
                });
            }
        });
        file_context.update(cx, |file_context, _| {
            file_context.git_changed_file_sets_task = Some(task);
        });
    }

    fn get_or_init_project(
        &mut self,
        project: &Entity<Project>,
        cx: &mut Context<Self>,
    ) -> &mut ProjectState {
        let entity_id = project.entity_id();
        self.projects
            .entry(entity_id)
            .or_insert_with(|| ProjectState {
                events: VecDeque::new(),
                last_event: None,
                recently_viewed_files: VecDeque::new(),
                recently_opened_files: VecDeque::new(),
                debug_tx: None,
                registered_buffers: HashMap::default(),
                file_contexts: HashMap::default(),
                current_prediction: None,
                last_edit_source: None,
                cancelled_predictions: HashSet::default(),
                pending_predictions: ArrayVec::new(),
                next_pending_prediction_id: 0,
                last_edit_prediction_refresh: None,
                license_detection_watchers: HashMap::default(),
                _subscriptions: [
                    cx.subscribe(&project, Self::handle_project_event),
                    cx.observe_release(&project, move |this, _, cx| {
                        this.projects.remove(&entity_id);
                        cx.notify();
                    }),
                ],
                copilot: None,
            })
    }

    pub fn remove_project(&mut self, project: &Entity<Project>) {
        self.projects.remove(&project.entity_id());
    }

    pub fn debug_info(
        &mut self,
        project: &Entity<Project>,
        cx: &mut Context<Self>,
    ) -> mpsc::UnboundedReceiver<DebugEvent> {
        let project_state = self.get_or_init_project(project, cx);
        let (debug_watch_tx, debug_watch_rx) = mpsc::unbounded();
        project_state.debug_tx = Some(debug_watch_tx);
        debug_watch_rx
    }

    fn handle_project_event(
        &mut self,
        project: Entity<Project>,
        event: &project::Event,
        cx: &mut Context<Self>,
    ) {
        if !is_ep_store_provider(all_language_settings(None, cx).edit_predictions.provider) {
            return;
        }
        // TODO [zeta2] init with recent paths
        match event {
            project::Event::BufferEdited { source } => {
                self.get_or_init_project(&project, cx).last_edit_source = Some(*source);
            }
            project::Event::ActiveEntryChanged(Some(active_entry_id)) => {
                let Some(project_state) = self.projects.get_mut(&project.entity_id()) else {
                    return;
                };
                let path = project.read(cx).path_for_entry(*active_entry_id, cx);
                if let Some(path) = path {
                    let cursor_position = project
                        .read(cx)
                        .buffer_store()
                        .read(cx)
                        .get_by_path(&path)
                        .and_then(|buffer| {
                            let position = project_state
                                .registered_buffers
                                .get(&buffer.entity_id())?
                                .last_position?;
                            Some(position.to_offset(&buffer.read(cx).snapshot()))
                        });

                    let recent_file = RecentFile {
                        path: path.path.as_std_path().into(),
                        cursor_position,
                    };
                    push_recent_file(&mut project_state.recently_viewed_files, recent_file);
                }
            }
            _ => (),
        }
    }

    fn register_buffer_impl<'a>(
        project_state: &'a mut ProjectState,
        buffer: &Entity<Buffer>,
        project: &Entity<Project>,
        cx: &mut Context<Self>,
    ) -> &'a mut RegisteredBuffer {
        let buffer_id = buffer.entity_id();

        if let Some(file) = buffer.read(cx).file() {
            let worktree_id = file.worktree_id(cx);
            if let Some(worktree) = project.read(cx).worktree_for_id(worktree_id, cx) {
                project_state
                    .license_detection_watchers
                    .entry(worktree_id)
                    .or_insert_with(|| {
                        let project_entity_id = project.entity_id();
                        cx.observe_release(&worktree, move |this, _worktree, _cx| {
                            let Some(project_state) = this.projects.get_mut(&project_entity_id)
                            else {
                                return;
                            };
                            project_state
                                .license_detection_watchers
                                .remove(&worktree_id);
                        })
                        .detach();
                        Rc::new(LicenseDetectionWatcher::new(&worktree, cx))
                    });
            }
        }

        match project_state.registered_buffers.entry(buffer_id) {
            hash_map::Entry::Occupied(entry) => entry.into_mut(),
            hash_map::Entry::Vacant(entry) => {
                let buf = buffer.read(cx);
                let snapshot = buf.text_snapshot();
                let file = buf.file().cloned();
                let project_entity_id = project.entity_id();
                entry.insert(RegisteredBuffer {
                    snapshot,
                    file,
                    last_position: None,
                    _subscriptions: [
                        cx.subscribe(buffer, {
                            let project = project.downgrade();
                            move |this, buffer, event, cx| {
                                if let language::BufferEvent::Edited { source } = event
                                    && let Some(project) = project.upgrade()
                                {
                                    let project_state = this.get_or_init_project(&project, cx);
                                    project_state.last_edit_source = Some(*source);
                                    this.report_changes_for_buffer(
                                        &buffer,
                                        &project,
                                        false,
                                        source.is_local(),
                                        cx,
                                    );
                                }
                            }
                        }),
                        cx.observe_release(buffer, move |this, _buffer, _cx| {
                            let Some(project_state) = this.projects.get_mut(&project_entity_id)
                            else {
                                return;
                            };
                            project_state.registered_buffers.remove(&buffer_id);
                        }),
                    ],
                })
            }
        }
    }

    fn report_changes_for_buffer(
        &mut self,
        buffer: &Entity<Buffer>,
        project: &Entity<Project>,
        is_predicted: bool,
        is_local: bool,
        cx: &mut Context<Self>,
    ) {
        let project_state = self.get_or_init_project(project, cx);
        let registered_buffer = Self::register_buffer_impl(project_state, buffer, project, cx);

        let buf = buffer.read(cx);
        let new_file = buf.file().cloned();
        let new_snapshot = buf.text_snapshot();
        if new_snapshot.version == registered_buffer.snapshot.version {
            return;
        }
        let old_file = mem::replace(&mut registered_buffer.file, new_file.clone());
        let old_snapshot = mem::replace(&mut registered_buffer.snapshot, new_snapshot.clone());
        let mut edit_range: Option<Range<Anchor>> = None;
        let now = cx.background_executor().now();

        for (_edit, anchor_range) in
            new_snapshot.anchored_edits_since::<usize>(&old_snapshot.version)
        {
            edit_range = Some(match edit_range {
                None => anchor_range,
                Some(acc) => acc.start..anchor_range.end,
            });
        }

        let Some(edit_range) = edit_range else {
            return;
        };

        let include_in_history = is_local
            || collaborator_edit_overlaps_locality_region(
                project_state,
                project,
                buffer,
                &buf.snapshot(),
                &edit_range,
                cx,
            );

        if !include_in_history {
            return;
        }

        let is_recordable_history_edit =
            compute_diff_between_snapshots_in_range(&old_snapshot, &new_snapshot, &edit_range)
                .is_some();

        if !is_recordable_history_edit {
            project_state.finalize_last_event(cx);
            return;
        }

        if let Some(last_event) = project_state.last_event.as_mut() {
            let is_next_snapshot_of_same_buffer = old_snapshot.remote_id()
                == last_event.new_snapshot.remote_id()
                && old_snapshot.version == last_event.new_snapshot.version;

            let prediction_source_changed = is_predicted != last_event.predicted;

            let should_coalesce = is_next_snapshot_of_same_buffer
                && !prediction_source_changed
                && lines_between_ranges(
                    &edit_range.to_point(&new_snapshot),
                    &last_event.latest_edit_range.to_point(&new_snapshot),
                ) <= CHANGE_GROUPING_LINE_SPAN;

            if should_coalesce {
                let pause_elapsed = last_event
                    .last_edit_time
                    .map(|t| now.duration_since(t) >= LAST_CHANGE_GROUPING_TIME)
                    .unwrap_or(false);
                if pause_elapsed {
                    last_event.snapshot_after_last_editing_pause =
                        Some(last_event.new_snapshot.clone());
                    last_event.total_edit_range_at_last_pause_boundary =
                        Some(last_event.total_edit_range.clone());
                }

                last_event.latest_edit_range = edit_range.clone();
                last_event.total_edit_range =
                    merge_anchor_ranges(&last_event.total_edit_range, &edit_range, &new_snapshot);
                last_event.new_snapshot = new_snapshot;
                last_event.last_edit_time = Some(now);
                return;
            }
        }

        project_state.finalize_last_event(cx);

        merge_trailing_events_if_needed(
            &mut project_state.events,
            &old_snapshot,
            &new_snapshot,
            &edit_range,
        );

        let file_context = new_file.as_ref().map(|file| {
            let project_path = ProjectPath::from_file(file.as_ref(), cx);
            let file_context = project_state.file_context_for_path(project_path.clone(), cx);
            Self::ensure_git_changed_file_sets_loading(&file_context, project, &project_path, cx);
            file_context
        });

        project_state.last_event = Some(LastEvent {
            old_file,
            new_file,
            old_snapshot,
            new_snapshot,
            latest_edit_range: edit_range.clone(),
            total_edit_range: edit_range,
            total_edit_range_at_last_pause_boundary: None,
            predicted: is_predicted,
            snapshot_after_last_editing_pause: None,
            last_edit_time: Some(now),
            file_context,
        });
    }

    fn prediction_at(
        &mut self,
        buffer: &Entity<Buffer>,
        position: Option<language::Anchor>,
        project: &Entity<Project>,
        cx: &App,
    ) -> Option<BufferEditPrediction<'_>> {
        let project_state = self.projects.get_mut(&project.entity_id())?;
        if let Some(position) = position {
            let snapshot = buffer.read(cx).snapshot();
            let cursor_position = position.to_offset(&snapshot);
            if let Some(file) = snapshot.file() {
                project_state.update_recent_file_cursor(file.path().as_std_path(), cursor_position);
            }
            if let Some(buffer) = project_state
                .registered_buffers
                .get_mut(&buffer.entity_id())
            {
                buffer.last_position = Some(position);
            }
        }

        let CurrentEditPrediction {
            requested_by,
            prediction,
            ..
        } = project_state.current_prediction.as_ref()?;

        if prediction.targets_buffer(buffer.read(cx)) {
            Some(BufferEditPrediction::Local { prediction })
        } else if requested_by == &buffer.entity_id() {
            Some(BufferEditPrediction::Jump { prediction })
        } else {
            None
        }
    }

    fn accept_current_prediction(&mut self, project: &Entity<Project>, cx: &mut Context<Self>) {
        let Some(current_prediction) = self
            .projects
            .get_mut(&project.entity_id())
            .and_then(|project_state| project_state.current_prediction.take())
        else {
            return;
        };

        self.report_changes_for_buffer(
            &current_prediction.prediction.buffer,
            project,
            true,
            true,
            cx,
        );

        // can't hold &mut project_state ref across report_changes_for_buffer_call
        let Some(project_state) = self.projects.get_mut(&project.entity_id()) else {
            return;
        };

        for pending_prediction in mem::take(&mut project_state.pending_predictions) {
            project_state.cancel_pending_prediction(pending_prediction, cx);
        }

        match self.edit_prediction_model {
            EditPredictionModel::Mercury => {
                mercury::edit_prediction_accepted(
                    current_prediction.prediction.id,
                    cx.http_client(),
                    cx,
                );
            }
            EditPredictionModel::Zeta
            | EditPredictionModel::Fim { .. }
            | EditPredictionModel::SweepPrompt => {}
        }
    }

    fn reject_current_prediction(
        &mut self,
        reason: EditPredictionRejectReason,
        project: &Entity<Project>,
        cx: &App,
    ) {
        if let Some(project_state) = self.projects.get_mut(&project.entity_id()) {
            project_state.pending_predictions.clear();
            if let Some(prediction) = project_state.current_prediction.take() {
                self.reject_prediction(prediction.prediction.id, reason, prediction.was_shown, cx);
            }
        };
    }

    fn did_show_current_prediction(
        &mut self,
        project: &Entity<Project>,
        display_type: edit_prediction_types::SuggestionDisplayType,
        _cx: &mut Context<Self>,
    ) {
        let Some(project_state) = self.projects.get_mut(&project.entity_id()) else {
            return;
        };

        let Some(current_prediction) = project_state.current_prediction.as_mut() else {
            return;
        };

        let is_jump = display_type == edit_prediction_types::SuggestionDisplayType::Jump;
        let previous_shown_with = current_prediction.shown_with;

        if previous_shown_with.is_none() || !is_jump {
            current_prediction.shown_with = Some(display_type);
        }

        let is_first_non_jump_show = !current_prediction.was_shown && !is_jump;

        if is_first_non_jump_show {
            current_prediction.was_shown = true;
        }
    }

    fn reject_prediction(
        &mut self,
        prediction_id: EditPredictionId,
        reason: EditPredictionRejectReason,
        was_shown: bool,
        cx: &App,
    ) {
        match self.edit_prediction_model {
            EditPredictionModel::Mercury => {
                mercury::edit_prediction_rejected(
                    prediction_id,
                    was_shown,
                    reason,
                    cx.http_client(),
                    cx,
                );
            }
            EditPredictionModel::Zeta
            | EditPredictionModel::SweepPrompt
            | EditPredictionModel::Fim { .. } => {}
        }
    }

    fn is_refreshing(&self, project: &Entity<Project>) -> bool {
        self.projects
            .get(&project.entity_id())
            .is_some_and(|project_state| !project_state.pending_predictions.is_empty())
    }

    pub fn refresh_prediction_from_buffer(
        &mut self,
        project: Entity<Project>,
        buffer: Entity<Buffer>,
        position: language::Anchor,
        debounce_duration: Duration,
        trigger: EditPredictionRequestTrigger,
        cx: &mut Context<Self>,
    ) {
        if currently_following(&project, cx) {
            return;
        }

        let trigger = predict_edits_request_trigger_from_editor_trigger(trigger);

        self.queue_prediction_refresh(
            project.clone(),
            buffer.entity_id(),
            debounce_duration,
            cx,
            move |this, cx| {
                let Some(request_task) = this
                    .update(cx, |this, cx| {
                        this.request_prediction_internal(
                            project.clone(),
                            buffer.clone(),
                            position,
                            trigger,
                            cx,
                        )
                    })
                    .log_err()
                else {
                    return Task::ready(anyhow::Ok(None));
                };

                cx.spawn(async move |_cx| {
                    request_task.await.map(|prediction_result| {
                        prediction_result
                            .map(|prediction_result| (prediction_result, buffer.entity_id()))
                    })
                })
            },
        )
    }

    pub const THROTTLE_TIMEOUT: Duration = Duration::from_millis(300);
}

fn currently_following(project: &Entity<Project>, cx: &App) -> bool {
    let Some(app_state) = AppState::try_global(cx) else {
        return false;
    };

    app_state
        .workspace_store
        .read(cx)
        .workspaces()
        .filter_map(|workspace| workspace.upgrade())
        .any(|workspace| {
            workspace.read(cx).project().entity_id() == project.entity_id()
                && workspace
                    .read(cx)
                    .leader_for_pane(workspace.read(cx).active_pane())
                    .is_some()
        })
}

fn is_ep_store_provider(provider: EditPredictionProvider) -> bool {
    match provider {
        EditPredictionProvider::Mercury
        | EditPredictionProvider::Ollama
        | EditPredictionProvider::OpenAiCompatibleApi => true,
        EditPredictionProvider::None
        | EditPredictionProvider::Copilot
        | EditPredictionProvider::Codestral => false,
    }
}

impl EditPredictionStore {
    fn queue_prediction_refresh(
        &mut self,
        project: Entity<Project>,
        throttle_entity: EntityId,
        debounce_duration: Duration,
        cx: &mut Context<Self>,
        do_refresh: impl FnOnce(
            WeakEntity<Self>,
            &mut AsyncApp,
        ) -> Task<Result<Option<(EditPredictionResult, EntityId)>>>
        + 'static,
    ) {
        let (needs_acceptance_tracking, max_pending_predictions) =
            match all_language_settings(None, cx).edit_predictions.provider {
                EditPredictionProvider::Mercury => (true, 2),
                EditPredictionProvider::Ollama => (false, 1),
                EditPredictionProvider::OpenAiCompatibleApi => (false, 2),
                EditPredictionProvider::None
                | EditPredictionProvider::Copilot
                | EditPredictionProvider::Codestral => {
                    log::error!("queue_prediction_refresh called with non-store provider");
                    return;
                }
            };

        let drop_on_cancel = !needs_acceptance_tracking;
        let throttle_timeout = Self::THROTTLE_TIMEOUT;
        let project_state = self.get_or_init_project(&project, cx);
        let pending_prediction_id = project_state.next_pending_prediction_id;
        project_state.next_pending_prediction_id += 1;
        let throttle_at_enqueue = project_state.last_edit_prediction_refresh;

        let task = cx.spawn(async move |this, cx| {
            if !debounce_duration.is_zero() {
                cx.background_executor().timer(debounce_duration).await;
            }

            let throttle_wait = this
                .update(cx, |this, cx| {
                    let project_state = this.get_or_init_project(&project, cx);
                    let throttle = project_state.last_edit_prediction_refresh;

                    let now = cx.background_executor().now();
                    throttle.and_then(|(last_entity, last_timestamp)| {
                        if throttle_entity != last_entity {
                            return None;
                        }
                        (last_timestamp + throttle_timeout).checked_duration_since(now)
                    })
                })
                .ok()
                .flatten();

            if let Some(timeout) = throttle_wait {
                cx.background_executor().timer(timeout).await;
            }

            // If this task was cancelled before the throttle timeout expired,
            // do not perform a request. Also skip if another task already
            // proceeded since we were enqueued (duplicate).
            let mut is_cancelled = true;
            this.update(cx, |this, cx| {
                let project_state = this.get_or_init_project(&project, cx);
                let was_cancelled = project_state
                    .cancelled_predictions
                    .remove(&pending_prediction_id);
                if was_cancelled {
                    return;
                }

                // Another request has been already sent since this was enqueued
                if project_state.last_edit_prediction_refresh != throttle_at_enqueue {
                    return;
                }

                let new_refresh = (throttle_entity, cx.background_executor().now());
                project_state.last_edit_prediction_refresh = Some(new_refresh);
                is_cancelled = false;
            })
            .ok();
            if is_cancelled {
                return None;
            }

            let new_prediction_result = do_refresh(this.clone(), cx).await.log_err().flatten();
            let new_prediction_id = new_prediction_result
                .as_ref()
                .map(|(result, _)| result.prediction.id.clone());

            // When a prediction completes, remove it from the pending list, and cancel
            // any pending predictions that were enqueued before it.
            this.update(cx, |this, cx| {
                let project_state = this.get_or_init_project(&project, cx);

                let is_cancelled = project_state
                    .cancelled_predictions
                    .remove(&pending_prediction_id);

                let new_current_prediction = if !is_cancelled
                    && let Some((prediction_result, requested_by)) = new_prediction_result
                {
                    let EditPredictionResult {
                        prediction,
                        reject_reason,
                        ..
                    } = prediction_result;

                    if let Some(reject_reason) = reject_reason {
                        this.reject_prediction(prediction.id, reject_reason, false, cx);

                        None
                    } else {
                        let new_prediction = CurrentEditPrediction {
                            requested_by,
                            prediction,
                            was_shown: false,
                            shown_with: None,
                        };

                        if let Some(current_prediction) = project_state.current_prediction.as_ref()
                        {
                            if new_prediction.should_replace_prediction(&current_prediction, cx) {
                                this.reject_current_prediction(
                                    EditPredictionRejectReason::Replaced,
                                    &project,
                                    cx,
                                );

                                Some(new_prediction)
                            } else {
                                this.reject_prediction(
                                    new_prediction.prediction.id,
                                    EditPredictionRejectReason::CurrentPreferred,
                                    false,
                                    cx,
                                );
                                None
                            }
                        } else {
                            Some(new_prediction)
                        }
                    }
                } else {
                    None
                };

                let project_state = this.get_or_init_project(&project, cx);

                if let Some(new_prediction) = new_current_prediction {
                    project_state.current_prediction = Some(new_prediction);
                }

                let mut pending_predictions = mem::take(&mut project_state.pending_predictions);
                for (ix, pending_prediction) in pending_predictions.iter().enumerate() {
                    if pending_prediction.id == pending_prediction_id {
                        pending_predictions.remove(ix);
                        for pending_prediction in pending_predictions.drain(0..ix) {
                            project_state.cancel_pending_prediction(pending_prediction, cx)
                        }
                        break;
                    }
                }
                this.get_or_init_project(&project, cx).pending_predictions = pending_predictions;
                cx.notify();
            })
            .ok();

            new_prediction_id
        });

        if project_state.pending_predictions.len() < max_pending_predictions {
            project_state
                .pending_predictions
                .push(PendingPrediction {
                    id: pending_prediction_id,
                    task,
                    drop_on_cancel,
                })
                .unwrap();
        } else {
            let pending_prediction = project_state.pending_predictions.pop().unwrap();
            project_state
                .pending_predictions
                .push(PendingPrediction {
                    id: pending_prediction_id,
                    task,
                    drop_on_cancel,
                })
                .unwrap();
            project_state.cancel_pending_prediction(pending_prediction, cx);
        }
    }

    pub fn request_prediction(
        &mut self,
        project: &Entity<Project>,
        active_buffer: &Entity<Buffer>,
        position: language::Anchor,
        trigger: PredictEditsRequestTrigger,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<EditPredictionResult>>> {
        self.request_prediction_internal(
            project.clone(),
            active_buffer.clone(),
            position,
            trigger,
            cx,
        )
    }

    fn request_prediction_internal(
        &mut self,
        project: Entity<Project>,
        active_buffer: Entity<Buffer>,
        position: language::Anchor,
        trigger: PredictEditsRequestTrigger,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<EditPredictionResult>>> {
        self.get_or_init_project(&project, cx);
        let (stored_events, debug_tx) = {
            let project_state = self.projects.get(&project.entity_id()).unwrap();
            (project_state.events(cx), project_state.debug_tx.clone())
        };
        let events: Vec<Arc<zeta_prompt::Event>> =
            stored_events.iter().map(|e| e.event.clone()).collect();

        let snapshot = active_buffer.read(cx).snapshot();
        let cursor_point = position.to_point(&snapshot);
        let diagnostic_search_start = cursor_point.row.saturating_sub(DIAGNOSTIC_LINES_RANGE);
        let diagnostic_search_end = cursor_point.row + DIAGNOSTIC_LINES_RANGE;
        let diagnostic_search_range =
            Point::new(diagnostic_search_start, 0)..Point::new(diagnostic_search_end, 0);

        let is_open_source = snapshot
            .file()
            .map_or(false, |file| self.is_file_open_source(&project, file, cx))
            && events.iter().all(|event| event.in_open_source_repo());

        let inputs = EditPredictionModelInput {
            buffer: active_buffer,
            snapshot,
            position,
            events,
            stored_events,
            related_files: Vec::new(),
            trigger,
            diagnostic_search_range,
            debug_tx,
            is_open_source,
        };

        let task = match self.edit_prediction_model {
            EditPredictionModel::Zeta => zeta::request_prediction_with_zeta(inputs, cx),
            EditPredictionModel::Fim { format } => fim::request_prediction(inputs, format, cx),
            EditPredictionModel::SweepPrompt => sweep_prompt::request_prediction(inputs, cx),
            EditPredictionModel::Mercury => {
                self.mercury
                    .request_prediction(inputs, self.credentials_provider.clone(), cx)
            }
        };

        task
    }

    pub fn collect_editable_context(
        &mut self,
        project: Entity<Project>,
        buffer: Entity<language::Buffer>,
        cursor_position: language::Anchor,
        oracle_targets: Vec<edit_prediction_context::OracleTarget>,
        context_sources: Vec<ContextSource>,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<Vec<RelatedFile>>> {
        use edit_prediction_context::{EditHistoryContextEntry, collect_editable_context};

        let buffers_by_id = project.read(cx).opened_buffers(cx).into_iter().fold(
            HashMap::default(),
            |mut buffers_by_id, buffer| {
                buffers_by_id.insert(buffer.read(cx).remote_id(), buffer.clone());
                buffers_by_id
            },
        );
        let edit_history = self
            .edit_history_for_project(&project, cx)
            .into_iter()
            .filter_map(|event| {
                let buffer = buffers_by_id.get(&event.old_snapshot.remote_id())?.clone();
                Some(EditHistoryContextEntry {
                    buffer,
                    edited_range: event.total_edit_range,
                })
            })
            .collect();

        cx.spawn(async move |_, cx| {
            collect_editable_context(
                project,
                buffer,
                cursor_position,
                edit_history,
                oracle_targets,
                context_sources,
                cx,
            )
            .await
        })
    }

    #[cfg(feature = "cli-support")]
    pub fn set_recent_paths_for_project(
        &mut self,
        project: &Entity<Project>,
        paths: impl IntoIterator<Item = project::ProjectPath>,
        cx: &mut Context<Self>,
    ) {
        let project_state = self.get_or_init_project(project, cx);
        project_state.recently_viewed_files = paths
            .into_iter()
            .map(|path| RecentFile {
                path: path.path.as_std_path().into(),
                cursor_position: None,
            })
            .collect();
    }

    pub fn recently_opened_files_for_project(&self, project: &Entity<Project>) -> Vec<RecentFile> {
        self.projects
            .get(&project.entity_id())
            .map(|project_state| {
                project_state
                    .recently_opened_files
                    .iter()
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn recently_viewed_files_for_project(&self, project: &Entity<Project>) -> Vec<RecentFile> {
        self.projects
            .get(&project.entity_id())
            .map(|project_state| {
                project_state
                    .recently_viewed_files
                    .iter()
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn is_file_open_source(
        &self,
        project: &Entity<Project>,
        file: &Arc<dyn File>,
        cx: &App,
    ) -> bool {
        if !file.is_local() || file.is_private() {
            return false;
        }
        let Some(project_state) = self.projects.get(&project.entity_id()) else {
            return false;
        };
        project_state
            .license_detection_watchers
            .get(&file.worktree_id(cx))
            .as_ref()
            .is_some_and(|watcher| watcher.is_project_open_source())
    }
}

fn collaborator_edit_overlaps_locality_region(
    project_state: &ProjectState,
    project: &Entity<Project>,
    buffer: &Entity<Buffer>,
    snapshot: &BufferSnapshot,
    edit_range: &Range<Anchor>,
    cx: &App,
) -> bool {
    let Some((active_buffer, Some(position))) = project_state.active_buffer(project, cx) else {
        return false;
    };

    if active_buffer.entity_id() != buffer.entity_id() {
        return false;
    }

    let locality_point_range = expand_context_syntactically_then_linewise(
        snapshot,
        (position..position).to_point(snapshot),
        COLLABORATOR_EDIT_LOCALITY_CONTEXT_TOKENS,
    );
    let locality_anchor_range = snapshot.anchor_range_inside(locality_point_range);

    edit_range.overlaps(&locality_anchor_range, snapshot)
}

fn merge_trailing_events_if_needed(
    events: &mut VecDeque<StoredEvent>,
    end_snapshot: &TextBufferSnapshot,
    latest_snapshot: &TextBufferSnapshot,
    latest_edit_range: &Range<Anchor>,
) {
    if let Some(last_event) = events.back() {
        if last_event.old_snapshot.remote_id() != latest_snapshot.remote_id() {
            return;
        }
        if !latest_snapshot
            .version
            .observed_all(&last_event.new_snapshot_version)
        {
            return;
        }
    }

    let mut next_old_event = None;
    let mut mergeable_count = 0;
    for old_event in events.iter().rev() {
        if let Some(next_old_event) = next_old_event
            && !old_event.can_merge(next_old_event, latest_snapshot, latest_edit_range)
        {
            break;
        }
        mergeable_count += 1;
        next_old_event = Some(old_event);
    }

    if mergeable_count <= 1 {
        return;
    }

    let merge_start = events.len() - mergeable_count;
    let oldest_event = &events[merge_start];
    let oldest_snapshot = oldest_event.old_snapshot.clone();
    let newest_snapshot = end_snapshot;
    let mut merged_edit_range = oldest_event.total_edit_range.clone();

    for event in events.range(events.len() - mergeable_count + 1..) {
        merged_edit_range =
            merge_anchor_ranges(&merged_edit_range, &event.total_edit_range, latest_snapshot);
    }

    if let Some((diff, old_range, new_range)) = compute_diff_between_snapshots_in_range(
        &oldest_snapshot,
        newest_snapshot,
        &merged_edit_range,
    ) {
        let merged_event = match oldest_event.event.as_ref() {
            zeta_prompt::Event::BufferChange {
                old_path,
                path,
                in_open_source_repo,
                ..
            } => StoredEvent {
                event: Arc::new(zeta_prompt::Event::BufferChange {
                    old_path: old_path.clone(),
                    path: path.clone(),
                    diff,
                    old_range,
                    new_range: new_range.clone(),
                    in_open_source_repo: *in_open_source_repo,
                    predicted: events.range(merge_start..).all(|event| {
                        matches!(
                            event.event.as_ref(),
                            zeta_prompt::Event::BufferChange {
                                predicted: true,
                                ..
                            }
                        )
                    }),
                }),
                old_snapshot: oldest_snapshot.clone(),
                new_snapshot_version: newest_snapshot.version.clone(),
                total_edit_range: newest_snapshot.anchor_before(new_range.start)
                    ..newest_snapshot.anchor_before(new_range.end),
                file_context: oldest_event.file_context.clone(),
            },
        };
        events.truncate(events.len() - mergeable_count);
        events.push_back(merged_event);
    }
}

fn merge_anchor_ranges(
    left: &Range<Anchor>,
    right: &Range<Anchor>,
    snapshot: &TextBufferSnapshot,
) -> Range<Anchor> {
    let start = if left.start.cmp(&right.start, snapshot).is_le() {
        left.start
    } else {
        right.start
    };
    let end = if left.end.cmp(&right.end, snapshot).is_ge() {
        left.end
    } else {
        right.end
    };
    start..end
}

pub fn init(cx: &mut App) {
    cx.observe_new(move |workspace: &mut Workspace, _, _cx| {
        fn copilot_for_project(project: &Entity<Project>, cx: &mut App) -> Option<Entity<Copilot>> {
            EditPredictionStore::try_global(cx).and_then(|store| {
                store.update(cx, |this, cx| this.start_copilot_for_project(project, cx))
            })
        }

        workspace.register_action(|workspace, _: &Reinstall, window, cx| {
            if let Some(copilot) = copilot_for_project(workspace.project(), cx) {
                copilot_ui::reinstall_and_sign_in(copilot, window, cx);
            }
        });
    })
    .detach();
}
