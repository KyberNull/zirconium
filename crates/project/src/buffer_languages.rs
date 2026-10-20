use crate::buffer_store::{BufferStore, BufferStoreEvent};
use futures::{FutureExt as _, StreamExt as _};
use gpui::{Context, Entity, EventEmitter, Task};
use language::{Bias, Buffer, BufferEvent, Language, LanguageRegistry, ModelineSettings, modeline};
use settings::SettingsStore;
use std::{cmp::Reverse, sync::Arc};

pub enum BufferLanguagesEvent {
    LanguageNotFound(Entity<Buffer>),
}

/// Keeps the language of every buffer in a [`BufferStore`] in sync with the
/// language registry, file names and modelines.
pub struct BufferLanguages {
    buffer_store: Entity<BufferStore>,
    languages: Arc<LanguageRegistry>,
    _maintain_buffer_languages: Task<()>,
}

impl EventEmitter<BufferLanguagesEvent> for BufferLanguages {}

impl BufferLanguages {
    pub fn new(
        buffer_store: Entity<BufferStore>,
        languages: Arc<LanguageRegistry>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&buffer_store, Self::on_buffer_store_event)
            .detach();
        Self {
            buffer_store,
            _maintain_buffer_languages: Self::maintain_buffer_languages(languages.clone(), cx),
            languages,
        }
    }

    fn on_buffer_store_event(
        &mut self,
        _: Entity<BufferStore>,
        event: &BufferStoreEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            BufferStoreEvent::BufferAdded(buffer) => {
                buffer
                    .read(cx)
                    .set_language_registry(self.languages.clone());

                cx.subscribe(buffer, |this, buffer, event, cx| {
                    if matches!(event, BufferEvent::Reloaded) && this.parse_modeline(&buffer, cx) {
                        this.detect_language_for_buffer(&buffer, cx);
                    }
                })
                .detach();

                self.parse_modeline(buffer, cx);
                self.detect_language_for_buffer(buffer, cx);
            }
            BufferStoreEvent::BufferChangedFilePath { buffer, .. } => {
                self.detect_language_for_buffer(buffer, cx);
            }
            _ => {}
        }
    }

    fn maintain_buffer_languages(
        languages: Arc<LanguageRegistry>,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let mut subscription = languages.subscribe();
        let mut prev_reload_count = languages.reload_count();
        cx.spawn(async move |this, cx| {
            while let Some(()) = subscription.next().await {
                let Some(this) = this.upgrade() else {
                    continue;
                };
                // If the language registry has been reloaded, then remove and
                // re-assign the languages on all open buffers.
                let reload_count = languages.reload_count();
                if reload_count > prev_reload_count {
                    prev_reload_count = reload_count;
                    this.update(cx, |this, cx| {
                        this.buffer_store.clone().update(cx, |buffer_store, cx| {
                            for buffer in buffer_store.buffers() {
                                if worktree::File::from_dyn(buffer.read(cx).file()).is_some() {
                                    buffer.update(cx, |buffer, cx| {
                                        buffer.set_language_async(None, cx)
                                    });
                                }
                            }
                        });
                    });
                }

                this.update(cx, |this, cx| {
                    let mut plain_text_buffers = Vec::new();
                    let mut buffers_with_unknown_injections = Vec::new();
                    for handle in this.buffer_store.read(cx).buffers() {
                        let buffer = handle.read(cx);
                        if buffer.language().is_none()
                            || buffer.language() == Some(&*language::PLAIN_TEXT)
                        {
                            plain_text_buffers.push(handle);
                        } else if buffer.contains_unknown_injections() {
                            buffers_with_unknown_injections.push(handle.clone());
                        }
                    }

                    // Visible worktrees come first so their buffers get their language earlier.
                    plain_text_buffers.sort_by_key(|buffer| {
                        Reverse(
                            worktree::File::from_dyn(buffer.read(cx).file())
                                .map(|file| file.worktree.read(cx).is_visible()),
                        )
                    });

                    for buffer in plain_text_buffers {
                        this.detect_language_for_buffer(&buffer, cx);
                    }

                    for buffer in buffers_with_unknown_injections {
                        buffer.update(cx, |buffer, cx| buffer.reparse(cx, false));
                    }
                });
            }
        })
    }

    fn parse_modeline(&mut self, buffer_handle: &Entity<Buffer>, cx: &mut Context<Self>) -> bool {
        let buffer = buffer_handle.read(cx);
        let content = buffer.as_rope();

        let modeline_settings = {
            let settings_store = cx.global::<SettingsStore>();
            let modeline_lines = settings_store
                .raw_user_settings()
                .and_then(|s| s.content.modeline_lines)
                .or(settings_store.raw_default_settings().modeline_lines)
                .unwrap_or(5);

            const MAX_MODELINE_BYTES: usize = 1024;

            let first_bytes =
                content.clip_offset(content.len().min(MAX_MODELINE_BYTES), Bias::Left);
            let mut first_lines = Vec::new();
            let mut lines = content.chunks_in_range(0..first_bytes).lines();
            for _ in 0..modeline_lines {
                if let Some(line) = lines.next() {
                    first_lines.push(line.to_string());
                } else {
                    break;
                }
            }
            let first_lines_ref: Vec<_> = first_lines.iter().map(|line| line.as_str()).collect();

            let last_start =
                content.clip_offset(content.len().saturating_sub(MAX_MODELINE_BYTES), Bias::Left);
            let mut last_lines = Vec::new();
            let mut lines = content
                .reversed_chunks_in_range(last_start..content.len())
                .lines();
            for _ in 0..modeline_lines {
                if let Some(line) = lines.next() {
                    last_lines.push(line.to_string());
                } else {
                    break;
                }
            }
            let last_lines_ref: Vec<_> =
                last_lines.iter().rev().map(|line| line.as_str()).collect();
            modeline::parse_modeline(&first_lines_ref, &last_lines_ref)
        };

        log::debug!("Parsed modeline settings: {:?}", modeline_settings);

        buffer_handle.update(cx, |buffer, _cx| buffer.set_modeline(modeline_settings))
    }

    fn detect_language_for_buffer(
        &mut self,
        buffer_handle: &Entity<Buffer>,
        cx: &mut Context<Self>,
    ) {
        let buffer = buffer_handle.read(cx);
        let Some(file) = buffer.file() else {
            return;
        };
        let content = buffer.as_rope();
        let modeline_settings = buffer.modeline().map(Arc::as_ref);

        let available_language = if let Some(ModelineSettings {
            mode: Some(mode_name),
            ..
        }) = modeline_settings
        {
            self.languages
                .available_language_for_modeline_name(mode_name)
                .map(|language| language.id())
        } else {
            self.languages.language_for_file(file, Some(content), cx)
        };
        if let Some(language_id) = available_language {
            if let Some(Ok(Ok(new_language))) =
                self.languages.load_language(language_id).now_or_never()
            {
                self.set_language_for_buffer(buffer_handle, new_language, cx);
            }
        } else {
            cx.emit(BufferLanguagesEvent::LanguageNotFound(
                buffer_handle.clone(),
            ));
        }
    }

    pub fn set_language_for_buffer(
        &mut self,
        buffer_entity: &Entity<Buffer>,
        new_language: Arc<Language>,
        cx: &mut Context<Self>,
    ) {
        buffer_entity.update(cx, |buffer, cx| {
            if buffer
                .language()
                .is_none_or(|old_language| !Arc::ptr_eq(old_language, &new_language))
            {
                buffer.set_language_async(Some(new_language), cx);
            }
        });
    }
}
