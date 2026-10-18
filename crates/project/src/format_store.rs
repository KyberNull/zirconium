use crate::{
    ProjectTransaction,
    buffer_store::BufferStore,
    environment::ProjectEnvironment,
    prettier_store::{self, PrettierStore},
    worktree_store::WorktreeStore,
};
use anyhow::{Context as _, Result};
use client::{TypedEnvelope, proto};
use collections::{BTreeMap, HashMap, HashSet};
use futures::AsyncWriteExt;
use gpui::{App, AsyncApp, Context, Entity, EventEmitter, Task, WeakEntity};
use itertools::Itertools as _;
use language::{
    Buffer, Diff, File as _, LineEnding, OffsetUtf16, ToOffset, ToOffsetUtf16, ToPoint as _,
    language_settings::{FormatOnSave, Formatter, LanguageSettings, LineEndingSetting},
    proto::{deserialize_anchor_range, serialize_anchor_range},
};
use rpc::AnyProtoClient;
use std::{ops::Range, path::PathBuf};
use text::{Anchor, BufferId};
use util::{debug_panic, defer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatTrigger {
    Save,
    Manual,
}

impl FormatTrigger {
    fn from_proto(value: i32) -> FormatTrigger {
        match value {
            0 => FormatTrigger::Save,
            1 => FormatTrigger::Manual,
            _ => FormatTrigger::Save,
        }
    }
}

pub enum FormatTarget {
    Buffers,
    Ranges(BTreeMap<BufferId, Vec<Range<Anchor>>>),
}

#[derive(Debug)]
struct FormattableBuffer {
    handle: Entity<Buffer>,
    abs_path: Option<PathBuf>,
    env: Option<HashMap<String, String>>,
    ranges: Option<Vec<Range<Anchor>>>,
}

struct LocalFormatStore {
    worktree_store: Entity<WorktreeStore>,
    environment: Entity<ProjectEnvironment>,
    prettier_store: Entity<PrettierStore>,
    buffers_being_formatted: HashSet<BufferId>,
}

struct RemoteFormatStore {
    upstream_client: AnyProtoClient,
    upstream_project_id: u64,
}

enum FormatStoreMode {
    Local(LocalFormatStore),
    Remote(RemoteFormatStore),
}

/// Formats buffers either locally (via Prettier or an external command) or by
/// forwarding the request to the host the buffers belong to.
pub struct FormatStore {
    mode: FormatStoreMode,
    buffer_store: Entity<BufferStore>,
    last_formatting_failure: Option<String>,
}

impl EventEmitter<()> for FormatStore {}

impl FormatStore {
    pub fn init(client: &AnyProtoClient) {
        client.add_entity_request_handler(Self::handle_format_buffers);
    }

    pub fn new_local(
        buffer_store: Entity<BufferStore>,
        worktree_store: Entity<WorktreeStore>,
        environment: Entity<ProjectEnvironment>,
        prettier_store: Entity<PrettierStore>,
    ) -> Self {
        Self {
            mode: FormatStoreMode::Local(LocalFormatStore {
                worktree_store,
                environment,
                prettier_store,
                buffers_being_formatted: HashSet::default(),
            }),
            buffer_store,
            last_formatting_failure: None,
        }
    }

    pub fn new_remote(
        buffer_store: Entity<BufferStore>,
        upstream_client: AnyProtoClient,
        upstream_project_id: u64,
    ) -> Self {
        Self {
            mode: FormatStoreMode::Remote(RemoteFormatStore {
                upstream_client,
                upstream_project_id,
            }),
            buffer_store,
            last_formatting_failure: None,
        }
    }

    pub fn last_formatting_failure(&self) -> Option<&str> {
        self.last_formatting_failure.as_deref()
    }

    pub fn reset_last_formatting_failure(&mut self) {
        self.last_formatting_failure = None;
    }

    fn update_last_formatting_failure<T>(&mut self, formatting_result: &Result<T>) {
        match formatting_result {
            Ok(_) => self.last_formatting_failure = None,
            Err(error) => {
                let error_string = format!("{error:#}");
                log::error!("Formatting failed: {error_string}");
                self.last_formatting_failure
                    .replace(error_string.lines().join(" "));
            }
        }
    }

    pub fn supports_range_formatting(&self, buffer: &Entity<Buffer>, cx: &App) -> bool {
        let settings = LanguageSettings::for_buffer(buffer.read(cx), cx);
        settings
            .formatter
            .as_ref()
            .iter()
            .any(|formatter| match formatter {
                Formatter::Auto => settings.prettier.allowed,
                Formatter::Prettier => true,
                Formatter::None
                | Formatter::External { .. }
                | Formatter::LanguageServer(_)
                | Formatter::CodeAction(_) => false,
            })
    }

    pub fn format(
        &mut self,
        buffers: HashSet<Entity<Buffer>>,
        target: FormatTarget,
        push_to_history: bool,
        trigger: FormatTrigger,
        cx: &mut Context<Self>,
    ) -> Task<Result<ProjectTransaction>> {
        let logger = zlog::scoped!("format");
        match &self.mode {
            FormatStoreMode::Local(local) => {
                zlog::trace!(logger => "Formatting locally");
                let logger = zlog::scoped!(logger => "local");
                let environment = local.environment.clone();
                let worktree_store = local.worktree_store.clone();
                let buffers = buffers
                    .into_iter()
                    .map(|buffer_handle| {
                        let buffer = buffer_handle.read(cx);
                        let buffer_abs_path = worktree::File::from_dyn(buffer.file())
                            .and_then(|file| file.as_local().map(|f| f.abs_path(cx)));

                        (buffer_handle, buffer_abs_path, buffer.remote_id())
                    })
                    .collect::<Vec<_>>();

                cx.spawn(async move |format_store, cx| {
                    let mut formattable_buffers = Vec::with_capacity(buffers.len());

                    for (handle, abs_path, id) in buffers {
                        let env = environment
                            .update(cx, |environment, cx| {
                                environment.buffer_environment(&handle, &worktree_store, cx)
                            })
                            .await;

                        let ranges = match &target {
                            FormatTarget::Buffers => None,
                            FormatTarget::Ranges(ranges) => Some(
                                ranges
                                    .get(&id)
                                    .context("No format ranges provided for buffer")?
                                    .clone(),
                            ),
                        };

                        formattable_buffers.push(FormattableBuffer {
                            handle,
                            abs_path,
                            env,
                            ranges,
                        });
                    }
                    zlog::trace!(logger => "Formatting {:?} buffers", formattable_buffers.len());

                    let format_timer = zlog::time!(logger => "Formatting buffers");
                    let result = Self::format_locally(
                        format_store.clone(),
                        formattable_buffers,
                        push_to_history,
                        trigger,
                        logger,
                        cx,
                    )
                    .await;
                    format_timer.end();

                    zlog::trace!(logger => "Formatting completed with result {:?}", result.as_ref().map(|_| "<project-transaction>"));

                    format_store.update(cx, |format_store, _| {
                        format_store.update_last_formatting_failure(&result);
                    })?;

                    result
                })
            }
            FormatStoreMode::Remote(remote) => {
                zlog::trace!(logger => "Formatting remotely");
                let logger = zlog::scoped!(logger => "remote");
                let client = remote.upstream_client.clone();
                let project_id = remote.upstream_project_id;

                let buffer_ranges = match &target {
                    FormatTarget::Buffers => Vec::new(),
                    FormatTarget::Ranges(ranges) => ranges
                        .iter()
                        .map(|(buffer_id, ranges)| proto::BufferFormatRanges {
                            buffer_id: buffer_id.to_proto(),
                            ranges: ranges.iter().cloned().map(serialize_anchor_range).collect(),
                        })
                        .collect(),
                };

                let buffer_store = self.buffer_store.clone();
                cx.spawn(async move |format_store, cx| {
                    zlog::trace!(logger => "Sending remote format request");
                    let request_timer = zlog::time!(logger => "remote format request");
                    let result = client
                        .request(proto::FormatBuffers {
                            project_id,
                            trigger: trigger as i32,
                            buffer_ids: buffers
                                .iter()
                                .map(|buffer| {
                                    buffer.read_with(cx, |buffer, _| buffer.remote_id().to_proto())
                                })
                                .collect(),
                            buffer_ranges,
                        })
                        .await
                        .and_then(|result| result.transaction.context("missing transaction"));
                    request_timer.end();

                    zlog::trace!(logger => "Remote format request resolved to {:?}", result.as_ref().map(|_| "<project_transaction>"));

                    format_store.update(cx, |format_store, _| {
                        format_store.update_last_formatting_failure(&result);
                    })?;

                    let transaction_response = result?;
                    let _timer = zlog::time!(logger => "deserializing project transaction");
                    buffer_store
                        .update(cx, |buffer_store, cx| {
                            buffer_store.deserialize_project_transaction(
                                transaction_response,
                                push_to_history,
                                cx,
                            )
                        })
                        .await
                })
            }
        }
    }

    async fn handle_format_buffers(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::FormatBuffers>,
        mut cx: AsyncApp,
    ) -> Result<proto::FormatBuffersResponse> {
        let sender_id = envelope.original_sender_id().unwrap_or_default();
        let format = this.update(&mut cx, |this, cx| {
            let mut buffers = HashSet::default();
            for buffer_id in &envelope.payload.buffer_ids {
                let buffer_id = BufferId::new(*buffer_id)?;
                buffers.insert(this.buffer_store.read(cx).get_existing(buffer_id)?);
            }

            let target = if envelope.payload.buffer_ranges.is_empty() {
                FormatTarget::Buffers
            } else {
                let mut ranges_map = BTreeMap::new();
                for buffer_range in &envelope.payload.buffer_ranges {
                    let buffer_id = BufferId::new(buffer_range.buffer_id)?;
                    let ranges: Result<Vec<_>> = buffer_range
                        .ranges
                        .iter()
                        .map(|range| {
                            deserialize_anchor_range(range.clone()).context("invalid anchor range")
                        })
                        .collect();
                    ranges_map.insert(buffer_id, ranges?);
                }
                FormatTarget::Ranges(ranges_map)
            };

            let trigger = FormatTrigger::from_proto(envelope.payload.trigger);
            anyhow::Ok(this.format(buffers, target, false, trigger, cx))
        })?;

        let project_transaction = format.await?;
        let project_transaction = this.update(&mut cx, |this, cx| {
            this.buffer_store.update(cx, |buffer_store, cx| {
                buffer_store.serialize_project_transaction_for_peer(
                    project_transaction,
                    sender_id,
                    cx,
                )
            })
        });
        Ok(proto::FormatBuffersResponse {
            transaction: Some(project_transaction),
        })
    }

    async fn format_locally(
        format_store: WeakEntity<FormatStore>,
        mut buffers: Vec<FormattableBuffer>,
        push_to_history: bool,
        trigger: FormatTrigger,
        logger: zlog::Logger,
        cx: &mut AsyncApp,
    ) -> Result<ProjectTransaction> {
        // Do not allow multiple concurrent formatting requests for the
        // same buffer.
        let prettier_store = format_store.update(cx, |this, cx| {
            let FormatStoreMode::Local(local) = &mut this.mode else {
                anyhow::bail!("local formatting requested on a remote format store");
            };
            buffers.retain(|buffer| {
                local
                    .buffers_being_formatted
                    .insert(buffer.handle.read(cx).remote_id())
            });
            Ok(local.prettier_store.downgrade())
        })??;

        let _cleanup = defer({
            let this = format_store.clone();
            let mut cx = cx.clone();
            let buffers = &buffers;
            move || {
                this.update(&mut cx, |this, cx| {
                    if let FormatStoreMode::Local(local) = &mut this.mode {
                        for buffer in buffers {
                            local
                                .buffers_being_formatted
                                .remove(&buffer.handle.read(cx).remote_id());
                        }
                    }
                })
                .ok();
            }
        });

        let mut project_transaction = ProjectTransaction::default();
        let mut format_error = None;

        for buffer in &buffers {
            zlog::debug!(
                logger =>
                "formatting buffer '{:?}'",
                buffer.abs_path.as_ref().unwrap_or(&PathBuf::from("unknown")).display()
            );
            // Create an empty transaction to hold all of the formatting edits.
            let formatting_transaction_id = buffer.handle.update(cx, |buffer, cx| {
                // ensure no transactions created while formatting are
                // grouped with the previous transaction in the history
                // based on the transaction group interval
                buffer.finalize_last_transaction();
                buffer
                    .start_transaction()
                    .context("transaction already open")?;
                buffer.end_transaction(cx);
                let transaction_id = buffer.push_empty_transaction(cx.background_executor().now());
                buffer.finalize_last_transaction();
                anyhow::Ok(transaction_id)
            })?;

            let result = Self::format_buffer_locally(
                &prettier_store,
                buffer,
                formatting_transaction_id,
                trigger,
                logger,
                cx,
            )
            .await;

            buffer.handle.update(cx, |buffer, cx| {
                let Some(formatting_transaction) =
                    buffer.get_transaction(formatting_transaction_id).cloned()
                else {
                    zlog::warn!(logger => "no formatting transaction");
                    return;
                };
                if formatting_transaction.edit_ids.is_empty() {
                    zlog::debug!(logger => "no changes made while formatting");
                    buffer.forget_transaction(formatting_transaction_id);
                    return;
                }
                if !push_to_history {
                    zlog::trace!(logger => "forgetting format transaction");
                    buffer.forget_transaction(formatting_transaction.id);
                }
                project_transaction
                    .0
                    .insert(cx.entity(), formatting_transaction);
            });

            // Keep formatting the remaining buffers on failure, surfacing the first error.
            if let Err(error) = result {
                format_error.get_or_insert(error);
            }
        }

        match format_error {
            Some(error) => Err(error),
            None => Ok(project_transaction),
        }
    }

    async fn format_buffer_locally(
        prettier_store: &WeakEntity<PrettierStore>,
        buffer: &FormattableBuffer,
        formatting_transaction_id: clock::Lamport,
        trigger: FormatTrigger,
        logger: zlog::Logger,
        cx: &mut AsyncApp,
    ) -> Result<()> {
        let settings = buffer.handle.update(cx, |buffer, cx| {
            LanguageSettings::for_buffer(buffer, cx).into_owned()
        });
        let had_existing_line_endings = buffer
            .handle
            .read_with(cx, |buffer, _| buffer.max_point().row > 0);

        // When formatting a selection, only the rows it spans may be touched.
        let selection_row_ranges = buffer.ranges.as_ref().map(|ranges| {
            buffer.handle.read_with(cx, |buffer, _cx| {
                let snapshot = buffer.snapshot();
                ranges
                    .iter()
                    .map(|range| {
                        let start = range.start.to_point(&snapshot);
                        let end = range.end.to_point(&snapshot);
                        // A selection ending at column 0 of a row only includes that row's
                        // preceding line break, not its content, so it shouldn't be trimmed.
                        let end_row = if end.column == 0 && end.row > start.row {
                            end.row
                        } else {
                            end.row + 1
                        };
                        start.row..end_row
                    })
                    .collect::<Vec<_>>()
            })
        });

        // handle whitespace formatting
        if settings.remove_trailing_whitespace_on_save {
            zlog::trace!(logger => "removing trailing whitespace");
            let diff = buffer
                .handle
                .read_with(cx, |buffer, cx| {
                    buffer.remove_trailing_whitespace(selection_row_ranges.as_deref(), cx)
                })
                .await;
            extend_formatting_transaction(buffer, formatting_transaction_id, cx, |buffer, cx| {
                buffer.apply_diff(diff, cx);
            })?;
        }

        if settings.ensure_final_newline_on_save {
            zlog::trace!(logger => "ensuring final newline");
            let diff = buffer.handle.read_with(cx, |buffer, _cx| {
                buffer.ensure_final_newline(selection_row_ranges.as_deref())
            });
            extend_formatting_transaction(buffer, formatting_transaction_id, cx, |buffer, cx| {
                buffer.apply_diff(diff, cx);
            })?;
        }

        let line_ending_policy = match settings.line_ending {
            LineEndingSetting::Detect => None,
            LineEndingSetting::PreferLf => Some((LineEnding::Unix, true)),
            LineEndingSetting::PreferCrlf => Some((LineEnding::Windows, true)),
            LineEndingSetting::EnforceLf => Some((LineEnding::Unix, false)),
            LineEndingSetting::EnforceCrlf => Some((LineEnding::Windows, false)),
        };
        if let Some((desired_line_ending, preserve_existing)) = line_ending_policy {
            buffer.handle.update(cx, |buffer, cx| {
                if buffer.line_ending() == desired_line_ending {
                    return;
                }
                if preserve_existing && had_existing_line_endings {
                    zlog::trace!(
                        logger => "preserving existing line endings ({}) on save",
                        buffer.line_ending().label()
                    );
                    return;
                }
                zlog::trace!(logger => "normalizing line endings to {}", desired_line_ending.label());
                buffer.set_line_ending(desired_line_ending, cx);
            });
        }

        let formatters = match (trigger, &settings.format_on_save) {
            (FormatTrigger::Save, FormatOnSave::Off) => &[],
            (FormatTrigger::Manual, _)
            | (
                FormatTrigger::Save,
                FormatOnSave::On
                | FormatOnSave::Modifications
                | FormatOnSave::ModificationsIfAvailable,
            ) => settings.formatter.as_ref(),
        };

        // Only explicitly configured formatters surface their failures;
        // auto-resolved ones stay silent so a missing optional tool
        // (e.g. prettier) does not warn on every save.
        let mut format_error = None;
        for formatter in formatters {
            let is_auto = formatter == &Formatter::Auto;
            let formatter = if is_auto {
                if settings.prettier.allowed {
                    zlog::trace!(logger => "Formatter set to auto: defaulting to prettier");
                    &Formatter::Prettier
                } else {
                    zlog::trace!(logger => "Formatter set to auto: prettier is not allowed, skipping");
                    &Formatter::None
                }
            } else {
                formatter
            };
            if let Err(err) = Self::apply_formatter(
                formatter,
                prettier_store,
                buffer,
                formatting_transaction_id,
                logger,
                cx,
            )
            .await
            {
                zlog::error!(logger => "Formatter failed, skipping: {err:#}");
                if !is_auto {
                    format_error.get_or_insert(err);
                }
            }
        }

        match format_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    async fn apply_formatter(
        formatter: &Formatter,
        prettier_store: &WeakEntity<PrettierStore>,
        buffer: &FormattableBuffer,
        formatting_transaction_id: clock::Lamport,
        logger: zlog::Logger,
        cx: &mut AsyncApp,
    ) -> Result<()> {
        match formatter {
            Formatter::None => {
                zlog::trace!(logger => "skipping formatter 'none'");
                return Ok(());
            }
            Formatter::Auto => {
                debug_panic!("Auto resolved above");
                return Ok(());
            }
            Formatter::Prettier => {
                let logger = zlog::scoped!(logger => "prettier");
                zlog::trace!(logger => "formatting");
                let _timer = zlog::time!(logger => "Formatting buffer via prettier");

                // When selection ranges are provided (via FormatSelections), we pass the
                // encompassing UTF-16 range to Prettier so it can scope its formatting.
                // After diffing, we filter the resulting edits to only keep those that
                // overlap with the original byte-level selection ranges.
                let (range_utf16, byte_ranges) = match buffer.ranges.as_ref() {
                    Some(ranges) if !ranges.is_empty() => {
                        let (utf16_range, byte_ranges) =
                            buffer.handle.read_with(cx, |buffer, _cx| {
                                let snapshot = buffer.snapshot();
                                let mut min_start_utf16 = OffsetUtf16(usize::MAX);
                                let mut max_end_utf16 = OffsetUtf16(0);
                                let mut byte_ranges = Vec::with_capacity(ranges.len());
                                for range in ranges {
                                    let start_utf16 = range.start.to_offset_utf16(&snapshot);
                                    let end_utf16 = range.end.to_offset_utf16(&snapshot);
                                    min_start_utf16.0 = min_start_utf16.0.min(start_utf16.0);
                                    max_end_utf16.0 = max_end_utf16.0.max(end_utf16.0);

                                    let start_byte = range.start.to_offset(&snapshot);
                                    let end_byte = range.end.to_offset(&snapshot);
                                    byte_ranges.push(start_byte..end_byte);
                                }
                                (min_start_utf16..max_end_utf16, byte_ranges)
                            });
                        (Some(utf16_range), Some(byte_ranges))
                    }
                    _ => (None, None),
                };

                let diff = prettier_store::format_with_prettier(
                    prettier_store,
                    &buffer.handle,
                    range_utf16,
                    cx,
                )
                .await
                .transpose()?;
                let Some(mut diff) = diff else {
                    zlog::trace!(logger => "No changes");
                    return Ok(());
                };

                if let Some(byte_ranges) = byte_ranges {
                    diff.edits.retain(|(edit_range, _)| {
                        byte_ranges.iter().any(|selection_range| {
                            edit_range.start < selection_range.end
                                && edit_range.end > selection_range.start
                        })
                    });
                    if diff.edits.is_empty() {
                        zlog::trace!(logger => "No changes within selection");
                        return Ok(());
                    }
                }

                extend_formatting_transaction(
                    buffer,
                    formatting_transaction_id,
                    cx,
                    |buffer, cx| {
                        buffer.apply_diff(diff, cx);
                    },
                )?;
            }
            Formatter::External { command, arguments } => {
                let logger = zlog::scoped!(logger => "command");

                if buffer.ranges.is_some() {
                    zlog::debug!(logger => "External formatter does not support range formatting; skipping");
                    return Ok(());
                }

                zlog::trace!(logger => "formatting");
                let _timer = zlog::time!(logger => "Formatting buffer via external command");

                let diff =
                    Self::format_via_external_command(buffer, command, arguments.as_deref(), cx)
                        .await
                        .with_context(|| {
                            format!("Failed to format buffer via external command: {}", command)
                        })?;
                let Some(diff) = diff else {
                    zlog::trace!(logger => "No changes");
                    return Ok(());
                };

                extend_formatting_transaction(
                    buffer,
                    formatting_transaction_id,
                    cx,
                    |buffer, cx| {
                        buffer.apply_diff(diff, cx);
                    },
                )?;
            }
            Formatter::LanguageServer(_) | Formatter::CodeAction(_) => {
                zlog::debug!(logger => "Language server formatters are no longer supported; skipping");
            }
        }
        Ok(())
    }

    async fn format_via_external_command(
        buffer: &FormattableBuffer,
        command: &str,
        arguments: Option<&[String]>,
        cx: &mut AsyncApp,
    ) -> Result<Option<Diff>> {
        let working_dir_path = buffer.handle.update(cx, |buffer, cx| {
            let file = worktree::File::from_dyn(buffer.file())?;
            let worktree = file.worktree.read(cx);
            let mut worktree_path = worktree.abs_path().to_path_buf();
            if worktree.root_entry()?.is_file() {
                worktree_path.pop();
            }
            Some(worktree_path)
        });

        use util::command::Stdio;
        let mut child = util::command::new_command(command);

        if let Some(buffer_env) = buffer.env.as_ref() {
            child.envs(buffer_env);
        }

        if let Some(working_dir_path) = working_dir_path {
            child.current_dir(working_dir_path);
        }

        if let Some(arguments) = arguments {
            child.args(arguments.iter().map(|arg| {
                if let Some(buffer_abs_path) = buffer.abs_path.as_ref() {
                    arg.replace("{buffer_path}", &buffer_abs_path.to_string_lossy())
                } else {
                    arg.replace("{buffer_path}", "Untitled")
                }
            }));
        }

        let mut child = child
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.as_mut().context("failed to acquire stdin")?;
        let text = buffer
            .handle
            .read_with(cx, |buffer, _| buffer.as_rope().clone());
        for chunk in text.chunks() {
            stdin.write_all(chunk.as_bytes()).await?;
        }
        stdin.flush().await?;

        let output = child.output().await?;
        anyhow::ensure!(
            output.status.success(),
            "command failed with exit code {:?}:\nstdout: {}\nstderr: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let stdout = String::from_utf8(output.stdout)?;
        if stdout.is_empty() && !text.is_empty() {
            // Some formatters (e.g. `cargo fmt`) rewrite files on disk instead of
            // printing the formatted contents to stdout. Treating empty stdout as the
            // new buffer contents would erase the buffer, so leave it untouched.
            log::warn!(
                "External formatter `{command}` produced no output on stdout; leaving the buffer unchanged"
            );
            return Ok(None);
        }

        Ok(Some(
            buffer
                .handle
                .update(cx, |buffer, cx| buffer.diff(stdout, cx))
                .await,
        ))
    }
}

fn extend_formatting_transaction(
    buffer: &FormattableBuffer,
    formatting_transaction_id: text::TransactionId,
    cx: &mut AsyncApp,
    operation: impl FnOnce(&mut Buffer, &mut Context<Buffer>),
) -> Result<()> {
    buffer.handle.update(cx, |buffer, cx| {
        let last_transaction_id = buffer.peek_undo_stack().map(|t| t.transaction_id());
        if last_transaction_id != Some(formatting_transaction_id) {
            anyhow::bail!("Buffer edited while formatting. Aborting")
        }
        buffer.start_transaction();
        operation(buffer, cx);
        if let Some(transaction_id) = buffer.end_transaction(cx) {
            buffer.merge_transactions(transaction_id, formatting_transaction_id);
        }
        Ok(())
    })
}
