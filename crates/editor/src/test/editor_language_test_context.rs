use std::{
    borrow::Cow,
    ops::{Deref, DerefMut},
    path::Path,
    sync::Arc,
};

use language::{markdown_lang, rust_lang};
use serde_json::json;

use crate::Editor;
use collections::HashSet;
use gpui::{Context, Entity, Focusable as _, VisualTestContext, Window};
use indoc::indoc;
use language::{BlockCommentConfig, Language, LanguageConfig, LanguageMatcher, LanguageQueries};
use project::Project;
use workspace::{AppState, MultiWorkspace, Workspace, WorkspaceHandle};

use super::editor_test_context::{AssertionContextManager, EditorTestContext};

pub struct EditorLanguageTestContext {
    pub cx: EditorTestContext,
    pub workspace: Entity<Workspace>,
}

#[cfg(test)]
pub(crate) fn git_commit_lang() -> Arc<Language> {
    Arc::new(Language::new(
        LanguageConfig {
            name: "Git Commit".into(),
            line_comments: vec!["#".into()],
            ..Default::default()
        },
        None,
    ))
}

impl EditorLanguageTestContext {
    pub async fn new(
        language: Language,
        cx: &mut gpui::TestAppContext,
    ) -> EditorLanguageTestContext {
        let app_state = cx.update(AppState::test);

        cx.update(|cx| {
            assets::Assets.load_test_fonts(cx);
            crate::init(cx);
            workspace::init(app_state.clone(), cx);
        });

        let file_name = format!(
            "file.{}",
            language
                .path_suffixes()
                .first()
                .expect("language must have a path suffix for EditorLanguageTestContext")
        );

        let project = Project::test(app_state.fs.clone(), [], cx).await;

        let language_registry = project.read_with(cx, |project, _| project.languages().clone());
        language_registry.add(Arc::new(language));

        let root = Self::root_path();

        app_state
            .fs
            .as_fake()
            .insert_tree(
                root,
                json!({
                    ".git": {},
                    "dir": {
                        file_name.clone(): ""
                    }
                }),
            )
            .await;

        let window =
            cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));

        let workspace = window.root(cx).unwrap();

        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        project
            .update(&mut cx, |project, cx| {
                project.find_or_create_worktree(root, true, cx)
            })
            .await
            .unwrap();
        cx.read(|cx| {
            workspace
                .read(cx)
                .workspace()
                .read(cx)
                .worktree_scans_complete(cx)
        })
        .await;
        let file = cx.read(|cx| workspace.read(cx).workspace().file_project_paths(cx)[0].clone());
        let item = workspace
            .update_in(&mut cx, |workspace, window, cx| {
                workspace.workspace().update(cx, |workspace, cx| {
                    workspace.open_path(file, None, true, window, cx)
                })
            })
            .await
            .expect("Could not open test file");
        let editor = cx.update(|_, cx| {
            item.act_as::<Editor>(cx)
                .expect("Opened test file wasn't an editor")
        });
        editor.update_in(&mut cx, |editor, window, cx| {
            let nav_history = workspace
                .read(cx)
                .workspace()
                .read(cx)
                .active_pane()
                .read(cx)
                .nav_history_for_item(&cx.entity());
            editor.set_nav_history(Some(nav_history));
            window.focus(&editor.focus_handle(cx), cx)
        });

        cx.executor().run_until_parked();

        let workspace = cx.read(|cx| workspace.read(cx).workspace().clone());

        Self {
            cx: EditorTestContext {
                cx,
                window: window.into(),
                editor,
                assertion_cx: AssertionContextManager::new(),
            },
            workspace,
        }
    }

    pub async fn new_rust(cx: &mut gpui::TestAppContext) -> EditorLanguageTestContext {
        Self::new(Arc::into_inner(rust_lang()).unwrap(), cx).await
    }

    pub async fn new_typescript(cx: &mut gpui::TestAppContext) -> EditorLanguageTestContext {
        let mut word_characters: HashSet<char> = Default::default();
        word_characters.insert('$');
        word_characters.insert('#');
        let language = Language::new(
            LanguageConfig {
                name: "Typescript".into(),
                matcher: (LanguageMatcher {
                    path_suffixes: vec!["ts".to_string()],
                    ..Default::default()
                }).into(),
                brackets: language::BracketPairConfig {
                    pairs: vec![language::BracketPair {
                        start: "{".to_string(),
                        end: "}".to_string(),
                        close: true,
                        surround: true,
                        newline: true,
                    }],
                    disabled_scopes_by_bracket_ix: Default::default(),
                },
                word_characters,
                ..Default::default()
            },
            Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        )
        .with_queries(LanguageQueries {
            brackets: Some(Cow::from(indoc! {r#"
                ("(" @open ")" @close)
                ("[" @open "]" @close)
                ("{" @open "}" @close)
                ("<" @open ">" @close)
                ("'" @open "'" @close)
                ("`" @open "`" @close)
                ("\"" @open "\"" @close)"#})),
            indents: Some(Cow::from(indoc! {r#"
                [
                    (call_expression)
                    (assignment_expression)
                    (member_expression)
                    (lexical_declaration)
                    (variable_declaration)
                    (assignment_expression)
                    (if_statement)
                    (for_statement)
                ] @indent

                (_ "[" "]" @end) @indent
                (_ "<" ">" @end) @indent
                (_ "{" "}" @end) @indent
                (_ "(" ")" @end) @indent
                "#})),
            text_objects: Some(Cow::from(indoc! {r#"
                (function_declaration
                    body: (_
                        "{"
                        (_)* @function.inside
                        "}")) @function.around

                (method_definition
                    body: (_
                        "{"
                        (_)* @function.inside
                        "}")) @function.around

                ; Arrow function in variable declaration - capture the full declaration
                ([
                    (lexical_declaration
                        (variable_declarator
                            value: (arrow_function
                                body: (statement_block
                                    "{"
                                    (_)* @function.inside
                                    "}"))))
                    (variable_declaration
                        (variable_declarator
                            value: (arrow_function
                                body: (statement_block
                                    "{"
                                    (_)* @function.inside
                                    "}"))))
                ]) @function.around

                ([
                    (lexical_declaration
                        (variable_declarator
                            value: (arrow_function)))
                    (variable_declaration
                        (variable_declarator
                            value: (arrow_function)))
                ]) @function.around

                ; Catch-all for arrow functions in other contexts (callbacks, etc.)
                ((arrow_function) @function.around (#not-has-parent? @function.around variable_declarator))
                "#})),
            ..Default::default()
        })
        .expect("Could not parse queries");

        Self::new(language, cx).await
    }

    pub async fn new_tsx(cx: &mut gpui::TestAppContext) -> EditorLanguageTestContext {
        let mut word_characters: HashSet<char> = Default::default();
        word_characters.insert('$');
        word_characters.insert('#');
        let language = Language::new(
            LanguageConfig {
                name: "TSX".into(),
                matcher: (LanguageMatcher {
                    path_suffixes: vec!["tsx".to_string()],
                    ..Default::default()
                }).into(),
                brackets: language::BracketPairConfig {
                    pairs: vec![language::BracketPair {
                        start: "{".to_string(),
                        end: "}".to_string(),
                        close: true,
                        surround: true,
                        newline: true,
                    }],
                    disabled_scopes_by_bracket_ix: Default::default(),
                },
                word_characters,
                ..Default::default()
            },
            Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        )
        .with_queries(LanguageQueries {
            brackets: Some(Cow::from(indoc! {r#"
                ("(" @open ")" @close)
                ("[" @open "]" @close)
                ("{" @open "}" @close)
                ("<" @open ">" @close)
                ("<" @open "/>" @close)
                ("</" @open ">" @close)
                ("\"" @open "\"" @close)
                ("'" @open "'" @close)
                ("`" @open "`" @close)
                ((jsx_element (jsx_opening_element) @open (jsx_closing_element) @close) (#set! newline.only))"#})),
            indents: Some(Cow::from(indoc! {r#"
                [
                    (call_expression)
                    (assignment_expression)
                    (member_expression)
                    (lexical_declaration)
                    (variable_declaration)
                    (assignment_expression)
                    (if_statement)
                    (for_statement)
                ] @indent

                (_ "[" "]" @end) @indent
                (_ "<" ">" @end) @indent
                (_ "{" "}" @end) @indent
                (_ "(" ")" @end) @indent

                (jsx_opening_element ">" @end) @indent

                (jsx_element
                  (jsx_opening_element) @start
                  (jsx_closing_element)? @end) @indent
                "#})),
            text_objects: Some(Cow::from(indoc! {r#"
                (function_declaration
                    body: (_
                        "{"
                        (_)* @function.inside
                        "}")) @function.around

                (method_definition
                    body: (_
                        "{"
                        (_)* @function.inside
                        "}")) @function.around

                ; Arrow function in variable declaration - capture the full declaration
                ([
                    (lexical_declaration
                        (variable_declarator
                            value: (arrow_function
                                body: (statement_block
                                    "{"
                                    (_)* @function.inside
                                    "}"))))
                    (variable_declaration
                        (variable_declarator
                            value: (arrow_function
                                body: (statement_block
                                    "{"
                                    (_)* @function.inside
                                    "}"))))
                ]) @function.around

                ([
                    (lexical_declaration
                        (variable_declarator
                            value: (arrow_function)))
                    (variable_declaration
                        (variable_declarator
                            value: (arrow_function)))
                ]) @function.around

                ; Catch-all for arrow functions in other contexts (callbacks, etc.)
                ((arrow_function) @function.around (#not-has-parent? @function.around variable_declarator))
                "#})),
            ..Default::default()
        })
        .expect("Could not parse queries");

        Self::new(language, cx).await
    }

    pub async fn new_html(cx: &mut gpui::TestAppContext) -> Self {
        let language = Language::new(
            LanguageConfig {
                name: "HTML".into(),
                matcher: (LanguageMatcher {
                    path_suffixes: vec!["html".into()],
                    ..Default::default()
                })
                .into(),
                block_comment: Some(BlockCommentConfig {
                    start: "<!--".into(),
                    prefix: "".into(),
                    end: "-->".into(),
                    tab_size: 0,
                }),
                completion_query_characters: ['-'].into_iter().collect(),
                ..Default::default()
            },
            Some(tree_sitter_html::LANGUAGE.into()),
        )
        .with_queries(LanguageQueries {
            brackets: Some(Cow::from(indoc! {r#"
                ("<" @open "/>" @close)
                ("</" @open ">" @close)
                ("<" @open ">" @close)
                ("\"" @open "\"" @close)"#})),
            ..Default::default()
        })
        .expect("Could not parse queries");
        Self::new(language, cx).await
    }

    pub async fn new_markdown_with_rust(cx: &mut gpui::TestAppContext) -> Self {
        let context = Self::new(Arc::into_inner(markdown_lang()).unwrap(), cx).await;

        let language_registry = context.workspace.read_with(cx, |workspace, cx| {
            workspace.project().read(cx).languages().clone()
        });
        language_registry.add(rust_lang());

        context
    }

    pub fn update_workspace<F, T>(&mut self, update: F) -> T
    where
        F: FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) -> T,
    {
        self.workspace.update_in(&mut self.cx.cx, update)
    }

    #[cfg(target_os = "windows")]
    pub fn root_path() -> &'static Path {
        Path::new("C:\\root")
    }

    #[cfg(not(target_os = "windows"))]
    pub fn root_path() -> &'static Path {
        Path::new("/root")
    }
}

impl Deref for EditorLanguageTestContext {
    type Target = EditorTestContext;

    fn deref(&self) -> &Self::Target {
        &self.cx
    }
}

impl DerefMut for EditorLanguageTestContext {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.cx
    }
}
