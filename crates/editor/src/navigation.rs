use super::*;

impl Editor {
    pub fn move_left(&mut self, _: &MoveLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                let cursor = if selection.is_empty() {
                    movement::left(map, selection.start)
                } else {
                    selection.start
                };
                selection.collapse_to(cursor, SelectionGoal::None);
            });
        })
    }

    pub fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| (movement::left(map, head), SelectionGoal::None));
        })
    }

    pub fn move_right(&mut self, _: &MoveRight, window: &mut Window, cx: &mut Context<Self>) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                let cursor = if selection.is_empty() {
                    movement::right(map, selection.end)
                } else {
                    selection.end
                };
                selection.collapse_to(cursor, SelectionGoal::None)
            });
        })
    }

    pub fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (movement::right(map, head), SelectionGoal::None)
            });
        });
    }

    pub fn move_up(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        let text_layout_details = &self.text_layout_details(window, cx);
        let selection_count = self.selections.count();
        let first_selection = self.selections.first_anchor();

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::up(
                    map,
                    selection.start,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });

        if selection_count == 1 && first_selection.range() == self.selections.first_anchor().range()
        {
            cx.propagate();
        }
    }

    pub fn move_up_by_lines(
        &mut self,
        action: &MoveUpByLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        let text_layout_details = &self.text_layout_details(window, cx);

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::up_by_rows(
                    map,
                    selection.start,
                    action.lines,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        })
    }

    pub fn move_down_by_lines(
        &mut self,
        action: &MoveDownByLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        let text_layout_details = &self.text_layout_details(window, cx);

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::down_by_rows(
                    map,
                    selection.start,
                    action.lines,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        })
    }

    pub fn select_down_by_lines(
        &mut self,
        action: &SelectDownByLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, goal| {
                movement::down_by_rows(map, head, action.lines, goal, false, text_layout_details)
            })
        })
    }

    pub fn select_up_by_lines(
        &mut self,
        action: &SelectUpByLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, goal| {
                movement::up_by_rows(map, head, action.lines, goal, false, text_layout_details)
            })
        })
    }

    pub fn select_page_up(
        &mut self,
        _: &SelectPageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row_count) = self.visible_row_count() else {
            return;
        };

        let text_layout_details = &self.text_layout_details(window, cx);

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, goal| {
                movement::up_by_rows(map, head, row_count, goal, false, text_layout_details)
            })
        })
    }

    pub fn move_page_up(
        &mut self,
        action: &MovePageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .context_menu
            .borrow_mut()
            .as_mut()
            .map(|menu| menu.select_first(self.completion_provider.as_deref(), window, cx))
            .unwrap_or(false)
        {
            return;
        }

        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }

        let Some(row_count) = self.visible_row_count() else {
            return;
        };

        let effects = if action.center_cursor {
            SelectionEffects::scroll(Autoscroll::center())
        } else {
            SelectionEffects::default()
        };

        let text_layout_details = &self.text_layout_details(window, cx);

        self.change_selections(effects, window, cx, |s| {
            s.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::up_by_rows(
                    map,
                    selection.end,
                    row_count,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });
    }

    pub fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, goal| {
                movement::up(map, head, goal, false, text_layout_details)
            })
        })
    }

    pub fn move_down(&mut self, _: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        let text_layout_details = &self.text_layout_details(window, cx);
        let selection_count = self.selections.count();
        let first_selection = self.selections.first_anchor();

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::down(
                    map,
                    selection.end,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });

        if selection_count == 1 && first_selection.range() == self.selections.first_anchor().range()
        {
            cx.propagate();
        }
    }

    pub fn select_page_down(
        &mut self,
        _: &SelectPageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row_count) = self.visible_row_count() else {
            return;
        };

        let text_layout_details = &self.text_layout_details(window, cx);

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, goal| {
                movement::down_by_rows(map, head, row_count, goal, false, text_layout_details)
            })
        })
    }

    pub fn move_page_down(
        &mut self,
        action: &MovePageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .context_menu
            .borrow_mut()
            .as_mut()
            .map(|menu| menu.select_last(self.completion_provider.as_deref(), window, cx))
            .unwrap_or(false)
        {
            return;
        }

        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }

        let Some(row_count) = self.visible_row_count() else {
            return;
        };

        let effects = if action.center_cursor {
            SelectionEffects::scroll(Autoscroll::center())
        } else {
            SelectionEffects::default()
        };

        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(effects, window, cx, |s| {
            s.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::down_by_rows(
                    map,
                    selection.end,
                    row_count,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });
    }

    pub fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, goal| {
                movement::down(map, head, goal, false, text_layout_details)
            })
        });
    }

    pub fn move_to_previous_word_start(
        &mut self,
        _: &MoveToPreviousWordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_cursors_with(&mut |map, head, _| {
                (
                    movement::previous_word_start(map, head, false),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_previous_subword_start(
        &mut self,
        _: &MoveToPreviousSubwordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_cursors_with(&mut |map, head, _| {
                (
                    movement::previous_subword_start(map, head),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_previous_word_start(
        &mut self,
        _: &SelectToPreviousWordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::previous_word_start(map, head, false),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_previous_subword_start(
        &mut self,
        _: &SelectToPreviousSubwordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::previous_subword_start(map, head),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_next_word_end(
        &mut self,
        _: &MoveToNextWordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_cursors_with(&mut |map, head, _| {
                (
                    movement::next_word_end(map, head, false),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_next_subword_end(
        &mut self,
        _: &MoveToNextSubwordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_cursors_with(&mut |map, head, _| {
                (movement::next_subword_end(map, head), SelectionGoal::None)
            });
        })
    }

    pub fn select_to_next_word_end(
        &mut self,
        _: &SelectToNextWordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::next_word_end(map, head, false),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_next_subword_end(
        &mut self,
        _: &SelectToNextSubwordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (movement::next_subword_end(map, head), SelectionGoal::None)
            });
        })
    }

    pub fn move_to_beginning_of_line(
        &mut self,
        action: &MoveToBeginningOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stop_at_indent = action.stop_at_indent && !self.mode.is_single_line();
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_cursors_with(&mut |map, head, _| {
                (
                    movement::indented_line_beginning(
                        map,
                        head,
                        action.stop_at_soft_wraps,
                        stop_at_indent,
                    ),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_beginning_of_line(
        &mut self,
        action: &SelectToBeginningOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stop_at_indent = action.stop_at_indent && !self.mode.is_single_line();
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::indented_line_beginning(
                        map,
                        head,
                        action.stop_at_soft_wraps,
                        stop_at_indent,
                    ),
                    SelectionGoal::None,
                )
            });
        });
    }

    pub fn move_to_end_of_line(
        &mut self,
        action: &MoveToEndOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_cursors_with(&mut |map, head, _| {
                (
                    movement::line_end(map, head, action.stop_at_soft_wraps),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_end_of_line(
        &mut self,
        action: &SelectToEndOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::line_end(map, head, action.stop_at_soft_wraps),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_start_of_paragraph(
        &mut self,
        _: &MoveToStartOfParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                selection.collapse_to(
                    movement::start_of_paragraph(map, selection.head(), 1),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_end_of_paragraph(
        &mut self,
        _: &MoveToEndOfParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                selection.collapse_to(
                    movement::end_of_paragraph(map, selection.head(), 1),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_next_comment_paragraph(
        &mut self,
        _: &MoveToNextCommentParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        // Keep the destination paragraph near the top of the viewport so the
        // whole paragraph below the caret stays visible after a jump.
        self.change_selections(
            SelectionEffects::scroll(Autoscroll::top_relative(5.0)),
            window,
            cx,
            |s| {
                s.move_with(&mut |map, selection| {
                    selection.collapse_to(
                        movement::comment_paragraph(
                            map,
                            selection.head(),
                            workspace::searchable::Direction::Next,
                        ),
                        SelectionGoal::None,
                    )
                });
            },
        )
    }

    pub fn move_to_previous_comment_paragraph(
        &mut self,
        _: &MoveToPreviousCommentParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        // Keep the destination paragraph near the top of the viewport so the
        // whole paragraph below the caret stays visible after a jump.
        self.change_selections(
            SelectionEffects::scroll(Autoscroll::top_relative(5.0)),
            window,
            cx,
            |s| {
                s.move_with(&mut |map, selection| {
                    selection.collapse_to(
                        movement::comment_paragraph(
                            map,
                            selection.head(),
                            workspace::searchable::Direction::Prev,
                        ),
                        SelectionGoal::None,
                    )
                });
            },
        )
    }

    pub fn select_to_start_of_paragraph(
        &mut self,
        _: &SelectToStartOfParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::start_of_paragraph(map, head, 1),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_end_of_paragraph(
        &mut self,
        _: &SelectToEndOfParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::end_of_paragraph(map, head, 1),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_start_of_excerpt(
        &mut self,
        _: &MoveToStartOfExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                selection.collapse_to(
                    movement::start_of_excerpt(
                        map,
                        selection.head(),
                        workspace::searchable::Direction::Prev,
                    ),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_start_of_next_excerpt(
        &mut self,
        _: &MoveToStartOfNextExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                selection.collapse_to(
                    movement::start_of_excerpt(
                        map,
                        selection.head(),
                        workspace::searchable::Direction::Next,
                    ),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_end_of_excerpt(
        &mut self,
        _: &MoveToEndOfExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                selection.collapse_to(
                    movement::end_of_excerpt(
                        map,
                        selection.head(),
                        workspace::searchable::Direction::Next,
                    ),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_end_of_previous_excerpt(
        &mut self,
        _: &MoveToEndOfPreviousExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                selection.collapse_to(
                    movement::end_of_excerpt(
                        map,
                        selection.head(),
                        workspace::searchable::Direction::Prev,
                    ),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_start_of_excerpt(
        &mut self,
        _: &SelectToStartOfExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::start_of_excerpt(map, head, workspace::searchable::Direction::Prev),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_start_of_next_excerpt(
        &mut self,
        _: &SelectToStartOfNextExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::start_of_excerpt(map, head, workspace::searchable::Direction::Next),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_end_of_excerpt(
        &mut self,
        _: &SelectToEndOfExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::end_of_excerpt(map, head, workspace::searchable::Direction::Next),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_end_of_previous_excerpt(
        &mut self,
        _: &SelectToEndOfPreviousExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::end_of_excerpt(map, head, workspace::searchable::Direction::Prev),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_beginning(
        &mut self,
        _: &MoveToBeginning,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.select_ranges(vec![Anchor::Min..Anchor::Min]);
        });
    }

    pub fn select_to_beginning(
        &mut self,
        _: &SelectToBeginning,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut selection = self.selections.last::<Point>(&self.display_snapshot(cx));
        selection.set_head(Point::zero(), SelectionGoal::None);
        self.change_selections(Default::default(), window, cx, |s| {
            s.select(vec![selection]);
        });
    }

    pub fn move_to_end(&mut self, _: &MoveToEnd, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        let cursor = self.buffer.read(cx).read(cx).len();
        self.change_selections(Default::default(), window, cx, |s| {
            s.select_ranges(vec![cursor..cursor])
        });
    }

    pub fn set_nav_history(&mut self, nav_history: Option<ItemNavHistory>) {
        self.nav_history = nav_history;
    }

    pub fn save_location(
        &mut self,
        _: &SaveLocation,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.create_nav_history_entry(cx);
    }

    pub fn create_nav_history_entry(&mut self, cx: &mut Context<Self>) {
        self.push_to_nav_history(
            self.selections.newest_anchor().head(),
            None,
            false,
            true,
            cx,
        );
    }

    pub fn expand_excerpts(
        &mut self,
        action: &ExpandExcerpts,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.expand_excerpts_for_direction(action.lines, ExpandExcerptDirection::UpAndDown, cx)
    }

    pub fn expand_excerpts_down(
        &mut self,
        action: &ExpandExcerptsDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.expand_excerpts_for_direction(action.lines, ExpandExcerptDirection::Down, cx)
    }

    pub fn expand_excerpts_up(
        &mut self,
        action: &ExpandExcerptsUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.expand_excerpts_for_direction(action.lines, ExpandExcerptDirection::Up, cx)
    }

    pub fn go_to_singleton_buffer_point(
        &mut self,
        point: Point,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go_to_singleton_buffer_range(point..point, window, cx);
    }

    pub fn go_to_singleton_buffer_range(
        &mut self,
        range: Range<Point>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go_to_singleton_buffer_range_impl(range, true, window, cx);
    }

    /// Like `go_to_singleton_buffer_point`, but does not push a navigation
    /// history entry. Useful when the caller already recorded one (e.g. when
    /// a file was just opened and we only need to move the cursor).
    pub fn go_to_singleton_buffer_point_silently(
        &mut self,
        point: Point,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go_to_singleton_buffer_range_impl(point..point, false, window, cx);
    }

    pub fn open_url(&mut self, _: &OpenUrl, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.selections.newest_anchor();
        let head = selection.head();
        let tail = selection.tail();

        let Some((buffer, start_position)) =
            self.buffer.read(cx).text_anchor_for_position(head, cx)
        else {
            return;
        };

        let end_position = if head != tail {
            let Some((_, pos)) = self.buffer.read(cx).text_anchor_for_position(tail, cx) else {
                return;
            };
            Some(pos)
        } else {
            None
        };

        let url_finder = cx.spawn_in(window, async move |_editor, cx| {
            let url = if let Some(end_pos) = end_position {
                find_url_from_range(&buffer, start_position..end_pos, cx)
            } else {
                find_url(&buffer, start_position, cx).map(|(_, url)| url)
            };

            if let Some(url) = url {
                cx.update(|window, cx| {
                    if parse_zed_link(&url, cx).is_some() {
                        window.dispatch_action(
                            Box::new(zed_actions::OpenZedUrl { url: url.into() }),
                            cx,
                        );
                    } else {
                        cx.open_url(&url);
                    }
                })?;
            }

            anyhow::Ok(())
        });

        url_finder.detach();
    }

    pub fn open_selected_filename(
        &mut self,
        _: &OpenSelectedFilename,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace() else {
            return;
        };

        let position = self.selections.newest_anchor().head();

        let Some((buffer, buffer_position)) =
            self.buffer.read(cx).text_anchor_for_position(position, cx)
        else {
            return;
        };

        let project = self.project.clone();

        cx.spawn_in(window, async move |_, cx| {
            let result = find_file(&buffer, project, buffer_position, cx).await;

            if let Some((_, file_target)) = result {
                let item = workspace
                    .update_in(cx, |workspace, window, cx| {
                        workspace.open_resolved_path(file_target.resolved_path.clone(), window, cx)
                    })?
                    .await?;

                file_target.navigate_item_to_position(item, cx);
            }
            anyhow::Ok(())
        })
        .detach();
    }

    pub(super) fn push_to_nav_history(
        &mut self,
        cursor_anchor: Anchor,
        new_position: Option<Point>,
        is_deactivate: bool,
        always: bool,
        cx: &mut Context<Self>,
    ) {
        let data = self.navigation_data(cursor_anchor, cx);
        if let Some(nav_history) = self.nav_history.as_mut() {
            if let Some(new_position) = new_position {
                let row_delta = (new_position.row as i64 - data.cursor_position.row as i64).abs();
                if row_delta == 0 || (row_delta < MIN_NAVIGATION_HISTORY_ROW_DELTA && !always) {
                    return;
                }
            }

            let cursor_row = data.cursor_position.row;
            nav_history.push(Some(data), Some(cursor_row), cx);
            cx.emit(EditorEvent::PushedToNavHistory {
                anchor: cursor_anchor,
                is_deactivate,
            })
        }
    }

    pub(super) fn expand_excerpt(
        &mut self,
        excerpt_anchor: Anchor,
        direction: ExpandExcerptDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lines_to_expand = EditorSettings::get_global(cx).expand_excerpt_lines;

        if self.delegate_expand_excerpts {
            cx.emit(EditorEvent::ExpandExcerptsRequested {
                excerpt_anchors: vec![excerpt_anchor],
                lines: lines_to_expand,
                direction,
            });
            return;
        }

        let current_scroll_position = self.scroll_position(cx);
        let mut scroll = None;

        if direction == ExpandExcerptDirection::Down {
            let multi_buffer = self.buffer.read(cx);
            let snapshot = multi_buffer.snapshot(cx);
            if let Some((buffer_snapshot, excerpt_range)) =
                snapshot.excerpt_containing(excerpt_anchor..excerpt_anchor)
            {
                let excerpt_end_row =
                    Point::from_anchor(&excerpt_range.context.end, &buffer_snapshot).row;
                let last_row = buffer_snapshot.max_point().row;
                let lines_below = last_row.saturating_sub(excerpt_end_row);
                if lines_below >= lines_to_expand {
                    scroll = Some(
                        current_scroll_position
                            + gpui::Point::new(0.0, lines_to_expand as ScrollOffset),
                    );
                }
            }
        }
        if direction == ExpandExcerptDirection::Up
            && self
                .buffer
                .read(cx)
                .snapshot(cx)
                .excerpt_before(excerpt_anchor)
                .is_none()
        {
            scroll = Some(current_scroll_position);
        }

        self.buffer.update(cx, |buffer, cx| {
            buffer.expand_excerpts([excerpt_anchor], lines_to_expand, direction, cx)
        });

        if let Some(new_scroll_position) = scroll {
            self.set_scroll_position(new_scroll_position, window, cx);
        }
    }

    pub(super) fn go_to_next_change(
        &mut self,
        _: &GoToNextChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(selections) = self
            .change_list
            .next_change(1, Direction::Next)
            .map(|s| s.to_vec())
        {
            self.change_selections(Default::default(), window, cx, |s| {
                let map = s.display_snapshot();
                s.select_display_ranges(selections.iter().map(|a| {
                    let point = a.to_display_point(&map);
                    point..point
                }))
            })
        }
    }

    pub(super) fn go_to_previous_change(
        &mut self,
        _: &GoToPreviousChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(selections) = self
            .change_list
            .next_change(1, Direction::Prev)
            .map(|s| s.to_vec())
        {
            self.change_selections(Default::default(), window, cx, |s| {
                let map = s.display_snapshot();
                s.select_display_ranges(selections.iter().map(|a| {
                    let point = a.to_display_point(&map);
                    point..point
                }))
            })
        }
    }

    pub(super) fn go_to_line<T: 'static>(
        &mut self,
        position: Anchor,
        highlight_color: fn(&App) -> Hsla,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let snapshot = self.snapshot(window, cx).display_snapshot;
        let position = position.to_point(&snapshot.buffer_snapshot());
        let start = snapshot
            .buffer_snapshot()
            .clip_point(Point::new(position.row, 0), Bias::Left);
        let end = start + Point::new(1, 0);
        let start = snapshot.buffer_snapshot().anchor_before(start);
        let end = snapshot.buffer_snapshot().anchor_before(end);

        self.highlight_rows::<T>(start..end, highlight_color, Default::default(), cx);

        if self.buffer.read(cx).is_singleton() {
            self.request_autoscroll(Autoscroll::center().for_anchor(start), cx);
        }
    }

    pub fn navigate_to_hover_links(
        &mut self,
        links: Vec<HoverLink>,
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) -> Task<Result<Navigated>> {
        let Some(link) = links.into_iter().next() else {
            return Task::ready(Ok(Navigated::No));
        };
        let workspace = self.workspace();
        cx.spawn_in(window, async move |_, cx| match link {
            HoverLink::Url(url) => {
                cx.update(|window, cx| {
                    if parse_zed_link(&url, cx).is_some() {
                        window.dispatch_action(
                            Box::new(zed_actions::OpenZedUrl { url: url.into() }),
                            cx,
                        );
                    } else {
                        cx.open_url(&url);
                    }
                })?;
                Ok(Navigated::Yes)
            }
            HoverLink::File(file_target) => {
                // TODO(andrew): respect preview tab settings
                //               `enable_keep_preview_on_code_navigation` and
                //               `enable_preview_file_from_code_navigation`
                let Some(workspace) = workspace else {
                    return Ok(Navigated::No);
                };
                let item = workspace
                    .update_in(cx, |workspace, window, cx| {
                        workspace.open_resolved_path(
                            file_target.resolved_path.clone(),
                            window,
                            cx,
                        )
                    })?
                    .await?;

                file_target.navigate_item_to_position(item, cx);

                Ok(Navigated::Yes)
            }
        })
    }

    pub(super) fn go_to_symbol_by_offset(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        offset: i8,
    ) -> Task<Result<()>> {
        let editor_snapshot = self.snapshot(window, cx);

        // We don't care about multi-buffer symbols
        if !editor_snapshot.is_singleton() {
            return Task::ready(Ok(()));
        }

        let cursor_offset = self
            .selections
            .newest::<MultiBufferOffset>(&editor_snapshot.display_snapshot)
            .head();

        cx.spawn_in(window, async move |editor, wcx| -> Result<()> {
            let Ok(Some(remote_id)) = editor.update(wcx, |ed, cx| {
                let buffer = ed.buffer.read(cx).as_singleton()?;
                Some(buffer.read(cx).remote_id())
            }) else {
                return Ok(());
            };

            let task = editor.update(wcx, |ed, cx| ed.buffer_outline_items(remote_id, cx))?;
            let outline_items: Vec<OutlineItem<text::Anchor>> = task.await;

            let multi_snapshot = editor_snapshot.buffer();
            let buffer_range = |range: &Range<_>| {
                Some(
                    multi_snapshot
                        .buffer_anchor_range_to_anchor_range(range.clone())?
                        .to_offset(multi_snapshot),
                )
            };

            wcx.update_window(wcx.window_handle(), |_, window, acx| {
                let current_idx = outline_items
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, item)| {
                        // Find the closest outline item by distance between outline text and cursor location
                        let source_range = buffer_range(&item.source_range_for_text)?;
                        let distance_to_closest_endpoint = cmp::min(
                            (source_range.start.0 as isize - cursor_offset.0 as isize).abs(),
                            (source_range.end.0 as isize - cursor_offset.0 as isize).abs(),
                        );

                        let item_towards_offset =
                            (source_range.start.0 as isize - cursor_offset.0 as isize).signum()
                                == (offset as isize).signum();

                        let source_range_contains_cursor = source_range.contains(&cursor_offset);

                        // To pick the next outline to jump to, we should jump in the direction of the offset, and
                        // we should not already be within the outline's source range. We then pick the closest outline
                        // item.
                        (item_towards_offset && !source_range_contains_cursor)
                            .then_some((distance_to_closest_endpoint, idx))
                    })
                    .min()
                    .map(|(_, idx)| idx);

                let Some(idx) = current_idx else {
                    return;
                };

                let Some(range) = buffer_range(&outline_items[idx].source_range_for_text) else {
                    return;
                };
                let selection = [range.start..range.start];

                editor
                    .update(acx, |editor, ecx| {
                        editor.change_selections(
                            SelectionEffects::scroll(Autoscroll::newest()),
                            window,
                            ecx,
                            |s| s.select_ranges(selection),
                        );
                    })
                    .log_err();
            })?;

            Ok(())
        })
    }

    pub(super) fn go_to_next_symbol(
        &mut self,
        _: &GoToNextSymbol,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go_to_symbol_by_offset(window, cx, 1).detach();
    }

    pub(super) fn go_to_previous_symbol(
        &mut self,
        _: &GoToPreviousSymbol,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go_to_symbol_by_offset(window, cx, -1).detach();
    }

    /// Opens a multibuffer with the given project locations in it.
    pub(super) fn open_locations_in_multibuffer(
        workspace: &mut Workspace,
        locations: std::collections::HashMap<Entity<Buffer>, Vec<Range<Point>>>,
        title: String,
        split: bool,
        allow_preview: bool,
        multibuffer_selection_mode: MultibufferSelectionMode,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Option<(Entity<Editor>, Entity<Pane>)> {
        if locations.is_empty() {
            log::error!("bug: open_locations_in_multibuffer called with empty list of locations");
            return None;
        }

        let capability = workspace.project().read(cx).capability();
        let mut ranges = <Vec<Range<Anchor>>>::new();

        // a key to find existing multibuffer editors with the same set of locations
        // to prevent us from opening more and more multibuffer tabs for searches and the like
        let mut key = (title.clone(), vec![]);
        let excerpt_buffer = cx.new(|cx| {
            let key = &mut key.1;
            let mut multibuffer = MultiBuffer::new(capability);
            for (buffer, mut ranges_for_buffer) in locations {
                ranges_for_buffer.sort_by_key(|range| (range.start, Reverse(range.end)));
                key.push((buffer.read(cx).remote_id(), ranges_for_buffer.clone()));
                multibuffer.set_excerpts_for_path(
                    PathKey::for_buffer(&buffer, cx),
                    buffer.clone(),
                    ranges_for_buffer.clone(),
                    multibuffer_context_lines(cx),
                    cx,
                );
                let snapshot = multibuffer.snapshot(cx);
                let buffer_snapshot = buffer.read(cx).snapshot();
                ranges.extend(ranges_for_buffer.into_iter().filter_map(|range| {
                    let text_range = buffer_snapshot.anchor_range_inside(range);
                    let start = snapshot.anchor_in_buffer(text_range.start)?;
                    let end = snapshot.anchor_in_buffer(text_range.end)?;
                    Some(start..end)
                }))
            }

            let final_snapshot = multibuffer.snapshot(cx);
            ranges.sort_by(|a, b| a.start.cmp(&b.start, &final_snapshot));

            multibuffer.with_title(title)
        });
        let existing = workspace.active_pane().update(cx, |pane, cx| {
            pane.items()
                .filter_map(|item| item.downcast::<Editor>())
                .find(|editor| {
                    editor
                        .read(cx)
                        .lookup_key
                        .as_ref()
                        .and_then(|it| {
                            it.downcast_ref::<(String, Vec<(BufferId, Vec<Range<Point>>)>)>()
                        })
                        .is_some_and(|it| *it == key)
                })
        });
        let was_existing = existing.is_some();
        let editor = existing.unwrap_or_else(|| {
            cx.new(|cx| {
                let mut editor = Editor::for_multibuffer(
                    excerpt_buffer,
                    Some(workspace.project().clone()),
                    window,
                    cx,
                );
                editor.lookup_key = Some(Box::new(key));
                editor
            })
        });
        editor.update(cx, |editor, cx| match multibuffer_selection_mode {
            MultibufferSelectionMode::First => {
                if let Some(first_range) = ranges.first() {
                    editor.change_selections(
                        SelectionEffects::no_scroll(),
                        window,
                        cx,
                        |selections| {
                            selections.clear_disjoint();
                            selections.select_anchor_ranges(std::iter::once(first_range.clone()));
                        },
                    );
                }
                editor.highlight_background(
                    HighlightKey::Editor,
                    &ranges,
                    |_, theme| theme.colors().editor_highlighted_line_background,
                    cx,
                );
            }
            MultibufferSelectionMode::All => {
                editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                    selections.clear_disjoint();
                    selections.select_anchor_ranges(ranges);
                });
            }
        });

        let item = Box::new(editor.clone());

        let pane = if split {
            workspace.adjacent_pane(window, cx)
        } else {
            workspace.active_pane().clone()
        };
        let activate_pane = split;

        let mut destination_index = None;
        pane.update(cx, |pane, cx| {
            if allow_preview && !was_existing {
                destination_index = pane.replace_preview_item_id(item.item_id(), window, cx);
                editor.update(cx, |editor, cx| {
                    editor
                        .buffer
                        .update(cx, |buffer, cx| buffer.refresh_preview(cx))
                });
            }
            if was_existing && !allow_preview {
                pane.unpreview_item_if_preview(item.item_id());
            }
            pane.add_item(item, activate_pane, true, destination_index, window, cx);
        });

        Some((editor, pane))
    }

    fn navigation_data(&self, cursor_anchor: Anchor, cx: &mut Context<Self>) -> NavigationData {
        let display_snapshot = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let buffer = self.buffer.read(cx).read(cx);
        let cursor_position = cursor_anchor.to_point(&buffer);
        let scroll_anchor = self.scroll_manager.native_anchor(&display_snapshot, cx);
        let scroll_top_row = scroll_anchor.top_row(&buffer);
        drop(buffer);

        NavigationData {
            cursor_anchor,
            cursor_position,
            scroll_anchor,
            scroll_top_row,
        }
    }

    fn expand_excerpts_for_direction(
        &mut self,
        lines: u32,
        direction: ExpandExcerptDirection,
        cx: &mut Context<Self>,
    ) {
        let selections = self.selections.disjoint_anchors_arc();

        let lines = if lines == 0 {
            EditorSettings::get_global(cx).expand_excerpt_lines
        } else {
            lines
        };

        let snapshot = self.buffer.read(cx).snapshot(cx);
        let excerpt_anchors = selections
            .iter()
            .flat_map(|selection| {
                snapshot
                    .range_to_buffer_ranges(selection.range())
                    .into_iter()
                    .filter_map(|(buffer_snapshot, range, _)| {
                        snapshot.anchor_in_excerpt(buffer_snapshot.anchor_after(range.start))
                    })
            })
            .collect::<Vec<_>>();

        if self.delegate_expand_excerpts {
            cx.emit(EditorEvent::ExpandExcerptsRequested {
                excerpt_anchors,
                lines,
                direction,
            });
            return;
        }

        self.buffer.update(cx, |buffer, cx| {
            buffer.expand_excerpts(excerpt_anchors, lines, direction, cx)
        })
    }

    fn go_to_singleton_buffer_range_impl(
        &mut self,
        range: Range<Point>,
        record_nav_history: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let multibuffer = self.buffer().read(cx);
        let Some(buffer) = multibuffer.as_singleton() else {
            return;
        };
        let Some(start) = multibuffer.buffer_point_to_anchor(&buffer, range.start, cx) else {
            return;
        };
        let Some(end) = multibuffer.buffer_point_to_anchor(&buffer, range.end, cx) else {
            return;
        };
        self.change_selections(
            SelectionEffects::scroll(Autoscroll::for_go_to_definition(
                self.cursor_top_offset(cx),
                cx,
            ))
            .nav_history(record_nav_history),
            window,
            cx,
            |s| s.select_anchor_ranges([start..end]),
        );
    }

}
