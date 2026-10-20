use crate::{
    DebugEvent, EditPredictionFinishedDebugEvent, EditPredictionId, EditPredictionInputs,
    EditPredictionModelInput, EditPredictionStartedDebugEvent, EditPredictionStore,
    buffer_path_with_id_fallback,
    cursor_excerpt::{self, compute_cursor_excerpt, compute_syntax_ranges},
    prediction::EditPredictionResult,
};
use anyhow::Result;
use edit_prediction_types::PredictedCursorPosition;
use gpui::{AppContext as _, Task, prelude::*};
use language::{
    BufferSnapshot, DiagnosticSeverity, EditPredictionPromptFormat, OffsetRangeExt as _,
    ToOffset as _, ZetaVersion, language_settings::all_language_settings, text_diff,
};
use text::{Anchor, Bias, Point};

use std::{ops::Range, path::Path, sync::Arc};
use zeta_prompt::{
    ParsedOutput, ZetaFormat, excerpt_ranges_for_format, format_zeta_prompt, get_prefill,
    parse_zeta2_model_output, stop_tokens_for_format,
    zeta1::{self, EDITABLE_REGION_END_MARKER},
};

use crate::open_ai_compatible::{
    load_open_ai_compatible_api_key_if_needed, send_custom_server_request,
};

pub(crate) fn request_prediction_with_zeta(
    EditPredictionModelInput {
        buffer,
        snapshot,
        position,
        related_files,
        events,
        debug_tx,
        trigger,
        diagnostic_search_range,
        is_open_source,
        ..
    }: EditPredictionModelInput,
    cx: &mut Context<EditPredictionStore>,
) -> Task<Result<Option<EditPredictionResult>>> {
    let settings = &all_language_settings(None, cx).edit_predictions;
    let provider = settings.provider;
    let custom_server_settings = match provider {
        settings::EditPredictionProvider::Ollama => settings.ollama.clone(),
        settings::EditPredictionProvider::OpenAiCompatibleApi => {
            settings.open_ai_compatible_api.clone()
        }
        _ => None,
    };
    let Some(custom_settings) = custom_server_settings else {
        return Task::ready(Ok(None));
    };

    let http_client = cx.http_client();
    let request_start = cx.background_executor().now();
    let open_ai_compatible_api_key = load_open_ai_compatible_api_key_if_needed(provider, cx);

    let excerpt_path = buffer_path_with_id_fallback(snapshot.file(), &snapshot.text, cx);

    struct ResolvedPrediction {
        request_id: EditPredictionId,
        inputs: EditPredictionInputs,
        edits: Vec<(Range<Anchor>, Arc<str>)>,
        cursor_position: Option<PredictedCursorPosition>,
        editable_range_in_buffer: Range<usize>,
    }

    let request_task = cx.background_spawn({
        let buffer = buffer.clone();
        let snapshot = snapshot.clone();
        async move {
            let local_zeta_version = match custom_settings.prompt_format {
                EditPredictionPromptFormat::Zeta(version) => Some(version),
                EditPredictionPromptFormat::Infer => {
                    match custom_settings.model.to_ascii_lowercase().as_str() {
                        "zeta" | "zeta1" => Some(ZetaVersion::Zeta1),
                        "zeta2" => Some(ZetaVersion::Zeta2),
                        "zeta2.1" => Some(ZetaVersion::Zeta2_1),
                        _ => None,
                    }
                }
                _ => None,
            }
            .unwrap_or_default();
            let zeta_format = match local_zeta_version {
                ZetaVersion::Zeta1 => None,
                ZetaVersion::Zeta2 => Some(ZetaFormat::V0211SeedCoder),
                ZetaVersion::Zeta2_1 => Some(ZetaFormat::V0318SeedMultiRegions),
            }
            .unwrap_or_default();

            let cursor_offset = position.to_offset(&snapshot);
            let (full_context_offset_range, prompt_input) = zeta2_prompt_input(
                &snapshot,
                related_files,
                events,
                diagnostic_search_range,
                excerpt_path,
                cursor_offset,
                is_open_source,
            );
            let formatted_prompt = format_zeta_prompt(&prompt_input, zeta_format);

            if let Some(debug_tx) = &debug_tx {
                debug_tx
                    .unbounded_send(DebugEvent::EditPredictionStarted(
                        EditPredictionStartedDebugEvent {
                            buffer: buffer.downgrade(),
                            prompt: formatted_prompt.clone(),
                            position,
                        },
                    ))
                    .ok();
            }

            log::trace!("Sending edit prediction request");

            let max_tokens = custom_settings.max_output_tokens * 4;
            let (request_id, output) = match local_zeta_version {
                ZetaVersion::Zeta1 => {
                    let ranges = &prompt_input.excerpt_ranges;
                    let editable_range_in_excerpt = ranges.editable_350.clone();
                    let prompt = zeta1::format_zeta1_from_input(
                        &prompt_input,
                        editable_range_in_excerpt.clone(),
                        ranges.editable_350_context_150.clone(),
                    );
                    let stop_tokens = vec![
                        EDITABLE_REGION_END_MARKER.to_string(),
                        format!("{EDITABLE_REGION_END_MARKER}\n"),
                        format!("{EDITABLE_REGION_END_MARKER}\n\n"),
                        format!("{EDITABLE_REGION_END_MARKER}\n\n\n"),
                    ];

                    let (response_text, request_id) = send_custom_server_request(
                        provider,
                        &custom_settings,
                        prompt,
                        max_tokens,
                        stop_tokens,
                        open_ai_compatible_api_key.clone(),
                        &http_client,
                    )
                    .await?;

                    let request_id = EditPredictionId(request_id.into());
                    let output_text = zeta1::clean_zeta1_model_output(&response_text);
                    let parsed_output = output_text.map(|text| ParsedOutput {
                        new_editable_region: text,
                        range_in_excerpt: editable_range_in_excerpt,
                        cursor_offset_in_new_editable_region: None,
                    });

                    (request_id, parsed_output)
                }
                ZetaVersion::Zeta2 | ZetaVersion::Zeta2_1 => {
                    let Some(prompt) = formatted_prompt else {
                        return Ok(None);
                    };
                    let prefill = get_prefill(&prompt_input, zeta_format);
                    let prompt = format!("{prompt}{prefill}");

                    let (response_text, request_id) = send_custom_server_request(
                        provider,
                        &custom_settings,
                        prompt,
                        max_tokens,
                        stop_tokens_for_format(zeta_format)
                            .iter()
                            .map(|token| token.to_string())
                            .collect(),
                        open_ai_compatible_api_key.clone(),
                        &http_client,
                    )
                    .await?;

                    let request_id = EditPredictionId(request_id.into());
                    let output_text = if response_text.is_empty() {
                        None
                    } else {
                        let output = format!("{prefill}{response_text}");
                        Some(parse_zeta2_model_output(
                            &output,
                            zeta_format,
                            &prompt_input,
                        )?)
                    };

                    (request_id, output_text)
                }
            };

            log::trace!("Got edit prediction response");

            let (new_editable_region, cursor_offset_in_output, editable_range_in_excerpt) =
                if let Some(output) = output {
                    let ParsedOutput {
                        new_editable_region: output_text,
                        range_in_excerpt: editable_range_in_excerpt,
                        cursor_offset_in_new_editable_region: cursor_offset_in_output,
                    } = output;
                    (
                        Some(output_text),
                        cursor_offset_in_output,
                        editable_range_in_excerpt,
                    )
                } else {
                    let (editable_range, _) =
                        excerpt_ranges_for_format(zeta_format, &prompt_input.excerpt_ranges);
                    (None, None, editable_range)
                };

            let editable_range_in_buffer = editable_range_in_excerpt.start
                + full_context_offset_range.start
                ..editable_range_in_excerpt.end + full_context_offset_range.start;

            if let Some(debug_tx) = &debug_tx {
                debug_tx
                    .unbounded_send(DebugEvent::EditPredictionFinished(
                        EditPredictionFinishedDebugEvent {
                            buffer: buffer.downgrade(),
                            position,
                            model_output: new_editable_region.clone(),
                        },
                    ))
                    .ok();
            }
            let (edits, cursor_position) = new_editable_region
                .map(|mut output_text| {
                    let mut old_text = snapshot
                        .text_for_range(editable_range_in_buffer.clone())
                        .collect::<String>();

                    if !output_text.is_empty() && !output_text.ends_with('\n') {
                        output_text.push('\n');
                    }
                    if !old_text.is_empty() && !old_text.ends_with('\n') {
                        old_text.push('\n');
                    }

                    compute_edits_and_cursor_position(
                        old_text,
                        &output_text,
                        editable_range_in_buffer.start,
                        cursor_offset_in_output,
                        &snapshot,
                    )
                })
                .unwrap_or_default();

            anyhow::Ok(Some(ResolvedPrediction {
                request_id,
                inputs: EditPredictionInputs::V2(prompt_input),
                edits,
                cursor_position,
                editable_range_in_buffer,
            }))
        }
    });

    cx.spawn(async move |_this, cx| {
        let Some(ResolvedPrediction {
            request_id,
            inputs,
            edits,
            cursor_position,
            editable_range_in_buffer,
        }) = request_task.await?
        else {
            return Ok(None);
        };
        let request_duration = cx.background_executor().now() - request_start;

        let result = EditPredictionResult::new(
            request_id,
            &buffer,
            &snapshot,
            edits.into(),
            cursor_position,
            Some(snapshot.anchor_range_inside(editable_range_in_buffer)),
            inputs,
            None,
            trigger,
            request_duration,
            cx,
        )
        .await;

        Ok(Some(result))
    })
}

const ACTIVE_BUFFER_DIAGNOSTIC_ADDITIONAL_CONTEXT_TOKEN_COUNT: usize = 100;
const MAX_ACTIVE_BUFFER_DIAGNOSTICS_TO_COLLECT: usize = 20;
pub(crate) const MAX_ACTIVE_BUFFER_DIAGNOSTIC_MESSAGE_TOKENS_TO_COLLECT: usize = 512;
pub(crate) const MAX_ACTIVE_BUFFER_DIAGNOSTIC_SNIPPET_TOKENS_TO_COLLECT: usize = 512;

pub(crate) fn active_buffer_diagnostics(
    snapshot: &language::BufferSnapshot,
    diagnostic_search_range: Range<Point>,
    cursor_row: u32,
    additional_context_token_count: usize,
) -> Vec<zeta_prompt::ActiveBufferDiagnostic> {
    let mut diagnostics = snapshot
        .diagnostics_in_range::<Point, Point>(diagnostic_search_range, false)
        .collect::<Vec<_>>();
    diagnostics.sort_by_key(|entry| {
        cursor_row.abs_diff(entry.range.start.row) + cursor_row.abs_diff(entry.range.end.row)
    });

    diagnostics
        .into_iter()
        .map(|entry| {
            let diagnostic_point_range = entry.range.clone();
            let snippet_point_range = cursor_excerpt::expand_context_syntactically_then_linewise(
                snapshot,
                diagnostic_point_range.clone(),
                additional_context_token_count,
            );

            let severity = match entry.diagnostic.severity {
                DiagnosticSeverity::ERROR => Some(1),
                DiagnosticSeverity::WARNING => Some(2),
                DiagnosticSeverity::INFORMATION => Some(3),
                DiagnosticSeverity::HINT => Some(4),
                _ => None,
            };
            (
                severity,
                zeta_prompt::clamp_text_to_token_count(
                    &entry.diagnostic.message,
                    MAX_ACTIVE_BUFFER_DIAGNOSTIC_MESSAGE_TOKENS_TO_COLLECT,
                )
                .to_string(),
                diagnostic_point_range,
                snippet_point_range,
            )
        })
        .take(MAX_ACTIVE_BUFFER_DIAGNOSTICS_TO_COLLECT)
        .map(
            |(severity, message, diagnostic_point_range, snippet_point_range)| {
                let (snippet, diagnostic_range_in_snippet) = if snippet_point_range.start
                    == Point::new(0, 0)
                    && snippet_point_range.end == snapshot.max_point()
                {
                    (String::new(), 0..0)
                } else {
                    let snippet = snapshot
                        .text_for_range(snippet_point_range.clone())
                        .collect::<String>();
                    let snippet = zeta_prompt::clamp_text_to_token_count(
                        &snippet,
                        MAX_ACTIVE_BUFFER_DIAGNOSTIC_SNIPPET_TOKENS_TO_COLLECT,
                    )
                    .to_string();
                    let snippet_start_offset = snippet_point_range.start.to_offset(snapshot);
                    let diagnostic_offset_range = diagnostic_point_range.to_offset(snapshot);
                    let diagnostic_range_start = diagnostic_offset_range
                        .start
                        .saturating_sub(snippet_start_offset)
                        .min(snippet.len());
                    let diagnostic_range_end = diagnostic_offset_range
                        .end
                        .saturating_sub(snippet_start_offset)
                        .min(snippet.len());
                    (snippet, diagnostic_range_start..diagnostic_range_end)
                };
                zeta_prompt::ActiveBufferDiagnostic {
                    severity,
                    message,
                    snippet,
                    snippet_buffer_row_range: diagnostic_point_range.start.row
                        ..diagnostic_point_range.end.row,
                    diagnostic_range_in_snippet,
                }
            },
        )
        .collect()
}

pub fn zeta2_prompt_input(
    snapshot: &language::BufferSnapshot,
    mut related_files: Vec<zeta_prompt::RelatedFile>,
    events: Vec<Arc<zeta_prompt::Event>>,
    diagnostic_search_range: Range<Point>,
    excerpt_path: Arc<Path>,
    cursor_offset: usize,
    is_open_source: bool,
) -> (Range<usize>, zeta_prompt::Zeta2PromptInput) {
    let (excerpt_point_range, excerpt_offset_range, cursor_offset_in_excerpt) =
        compute_cursor_excerpt(snapshot, cursor_offset);

    let cursor_excerpt: Arc<str> = snapshot
        .text_for_range(excerpt_point_range.clone())
        .collect::<String>()
        .into();
    let syntax_ranges = compute_syntax_ranges(snapshot, cursor_offset, &excerpt_offset_range);
    let excerpt_ranges = zeta_prompt::compute_legacy_excerpt_ranges(
        &cursor_excerpt,
        cursor_offset_in_excerpt,
        &syntax_ranges,
    );

    let active_buffer_diagnostics = active_buffer_diagnostics(
        snapshot,
        diagnostic_search_range,
        snapshot.offset_to_point(cursor_offset).row,
        ACTIVE_BUFFER_DIAGNOSTIC_ADDITIONAL_CONTEXT_TOKEN_COUNT,
    );
    for file in &mut related_files {
        for excerpt in &mut file.excerpts {
            excerpt.context_source = zeta_prompt::ContextSource::Lsp;
        }
    }

    let prompt_input = zeta_prompt::Zeta2PromptInput {
        cursor_path: excerpt_path,
        cursor_excerpt,
        cursor_offset_in_excerpt,
        excerpt_start_row: Some(excerpt_point_range.start.row),
        events,
        related_files: Some(related_files),
        active_buffer_diagnostics,
        excerpt_ranges,
        syntax_ranges: Some(syntax_ranges),
        in_open_source_repo: is_open_source,
        can_collect_data: false,
        repo_url: None,
    };
    (excerpt_offset_range, prompt_input)
}

pub fn compute_edits(
    old_text: String,
    new_text: &str,
    offset: usize,
    snapshot: &BufferSnapshot,
) -> Vec<(Range<Anchor>, Arc<str>)> {
    compute_edits_and_cursor_position(old_text, new_text, offset, None, snapshot).0
}

pub fn compute_edits_and_cursor_position(
    old_text: String,
    new_text: &str,
    offset: usize,
    cursor_offset_in_new_text: Option<usize>,
    snapshot: &BufferSnapshot,
) -> (
    Vec<(Range<Anchor>, Arc<str>)>,
    Option<PredictedCursorPosition>,
) {
    let diffs = text_diff(&old_text, new_text);

    // Delta represents the cumulative change in byte count from all preceding edits.
    // new_offset = old_offset + delta, so old_offset = new_offset - delta
    let mut delta: isize = 0;
    let mut cursor_position: Option<PredictedCursorPosition> = None;
    let buffer_len = snapshot.len();

    let edits = diffs
        .iter()
        .map(|(raw_old_range, new_text)| {
            // Compute cursor position if it falls within or before this edit.
            if let (Some(cursor_offset), None) = (cursor_offset_in_new_text, cursor_position) {
                let edit_start_in_new = (raw_old_range.start as isize + delta) as usize;
                let edit_end_in_new = edit_start_in_new + new_text.len();

                if cursor_offset < edit_start_in_new {
                    let cursor_in_old = (cursor_offset as isize - delta) as usize;
                    let buffer_offset = (offset + cursor_in_old).min(buffer_len);
                    cursor_position = Some(PredictedCursorPosition::at_anchor(
                        snapshot.anchor_after(buffer_offset),
                    ));
                } else if cursor_offset < edit_end_in_new {
                    let buffer_offset = (offset + raw_old_range.start).min(buffer_len);
                    let offset_within_insertion = cursor_offset - edit_start_in_new;
                    cursor_position = Some(PredictedCursorPosition::new(
                        snapshot.anchor_before(buffer_offset),
                        offset_within_insertion,
                    ));
                }

                delta += new_text.len() as isize - raw_old_range.len() as isize;
            }

            // Compute the edit with prefix/suffix trimming.
            let mut old_range = raw_old_range.clone();
            let old_slice = &old_text[old_range.clone()];

            let prefix_len = common_prefix(old_slice.chars(), new_text.chars());
            let suffix_len = common_prefix(
                old_slice[prefix_len..].chars().rev(),
                new_text[prefix_len..].chars().rev(),
            );

            old_range.start += offset;
            old_range.end += offset;
            old_range.start += prefix_len;
            old_range.end -= suffix_len;

            old_range.start = old_range.start.min(buffer_len);
            old_range.end = old_range.end.min(buffer_len);

            let new_text = new_text[prefix_len..new_text.len() - suffix_len].into();
            let range = if old_range.is_empty() {
                let anchor = snapshot.anchor_after(old_range.start);
                anchor..anchor
            } else {
                snapshot.anchor_after(old_range.start)..snapshot.anchor_before(old_range.end)
            };
            (range, new_text)
        })
        .collect();

    if let (Some(cursor_offset), None) = (cursor_offset_in_new_text, cursor_position) {
        let cursor_in_old = (cursor_offset as isize - delta) as usize;
        let buffer_offset = snapshot.clip_offset(offset + cursor_in_old, Bias::Right);
        cursor_position = Some(PredictedCursorPosition::at_anchor(
            snapshot.anchor_after(buffer_offset),
        ));
    }

    (edits, cursor_position)
}

fn common_prefix<T1: Iterator<Item = char>, T2: Iterator<Item = char>>(a: T1, b: T2) -> usize {
    a.zip(b)
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum()
}
