use anyhow::Result;
use collections::HashMap;
use gpui::{App, Entity, Task};
pub use language::*;
use language::LanguageToolchainStore;

use regex::Regex;
use std::{
    borrow::Cow,
    str,
    sync::{
        Arc, LazyLock,
    },
};
use task::{TaskTemplate, TaskTemplates, TaskVariables, VariableName};

static GO_ESCAPE_SUBTEST_NAME_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"[.*+?^${}()|\[\]\\"']"#).expect("Failed to create GO_ESCAPE_SUBTEST_NAME_REGEX")
});

pub(crate) struct GoContextProvider;

pub(crate) struct GoRunnableResolver;

impl RunnableResolver for GoRunnableResolver {
    fn resolve(
        &self,
        local_captures: &[RunnableMatchCapture],
        shared_captures: &[RunnableMatchCapture],
        buffer: &BufferSnapshot,
    ) -> Option<ResolvedRunnable> {
        const FIELD_CHECK: &str = "_field_check";
        const FIELD_NAME: &str = "_field_name";
        const TABLE_TEST_CASE_NAME: &str = "_table_test_case_name";

        // A row may declare several string fields (e.g. `{ name: "x", label: "y" }`), so
        // the query emits one `@_field_name` + `@run` pair per field, in source order.
        //
        // When the loop body calls `t.Run(tc.<field>, ...)`, `@_field_check` names that
        // field; pick the pair whose `@_field_name` matches it. Without a `@_field_check`
        // (e.g. map-keyed tables) the first pair wins.
        let reference_text = shared_captures
            .iter()
            .find(|capture| capture.name() == Some(FIELD_CHECK))
            .map(|capture| buffer.text_for_range(capture.range()).collect::<String>());
        let pair_index = match &reference_text {
            Some(reference) => local_captures
                .iter()
                .filter(|capture| capture.name() == Some(FIELD_NAME))
                .position(|capture| buffer.text_for_range(capture.range()).equals_str(reference))?,
            None => 0,
        };

        // `@run` and `@_table_test_case_name` tag the same string literal, so the chosen
        // run's text is the case name.
        let run_capture = local_captures
            .iter()
            .filter(|capture| capture.is_run())
            .nth(pair_index)?;
        Some(ResolvedRunnable {
            run_range: run_capture.range(),
            extra_captures: smallvec::smallvec![(
                TABLE_TEST_CASE_NAME.to_string(),
                buffer.text_for_range(run_capture.range()).collect(),
            )],
        })
    }
}

const GO_PACKAGE_TASK_VARIABLE: VariableName = VariableName::Custom(Cow::Borrowed("GO_PACKAGE"));
const GO_MODULE_ROOT_TASK_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("GO_MODULE_ROOT"));
const GO_SUBTEST_NAME_TASK_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("GO_SUBTEST_NAME"));
const GO_TABLE_TEST_CASE_NAME_TASK_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("GO_TABLE_TEST_CASE_NAME"));
const GO_SUITE_NAME_TASK_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("GO_SUITE_NAME"));

impl ContextProvider for GoContextProvider {
    fn build_context(
        &self,
        variables: &TaskVariables,
        location: ContextLocation<'_>,
        _: Option<HashMap<String, String>>,
        _: Arc<dyn LanguageToolchainStore>,
        cx: &mut gpui::App,
    ) -> Task<Result<TaskVariables>> {
        let local_abs_path = location
            .file_location
            .buffer
            .read(cx)
            .file()
            .and_then(|file| Some(file.as_local()?.abs_path(cx)));

        let go_package_variable = local_abs_path
            .as_deref()
            .and_then(|local_abs_path| local_abs_path.parent())
            .map(|buffer_dir| {
                // Prefer the relative form `./my-nested-package/is-here` over
                // absolute path, because it's more readable in the modal, but
                // the absolute path also works.
                let package_name = variables
                    .get(&VariableName::WorktreeRoot)
                    .and_then(|worktree_abs_path| buffer_dir.strip_prefix(worktree_abs_path).ok())
                    .map(|relative_pkg_dir| {
                        if relative_pkg_dir.as_os_str().is_empty() {
                            ".".into()
                        } else {
                            format!("./{}", relative_pkg_dir.to_string_lossy())
                        }
                    })
                    .unwrap_or_else(|| format!("{}", buffer_dir.to_string_lossy()));

                (GO_PACKAGE_TASK_VARIABLE.clone(), package_name)
            });

        let go_module_root_variable = local_abs_path
            .as_deref()
            .and_then(|local_abs_path| local_abs_path.parent())
            .map(|buffer_dir| {
                // Walk dirtree up until getting the first go.mod file
                let module_dir = buffer_dir
                    .ancestors()
                    .find(|dir| dir.join("go.mod").is_file())
                    .map(|dir| dir.to_string_lossy().into_owned())
                    .unwrap_or_else(|| ".".to_string());

                (GO_MODULE_ROOT_TASK_VARIABLE.clone(), module_dir)
            });

        let _subtest_name = variables.get(&VariableName::Custom(Cow::Borrowed("_subtest_name")));

        let go_subtest_variable = extract_subtest_name(_subtest_name.unwrap_or(""))
            .map(|subtest_name| (GO_SUBTEST_NAME_TASK_VARIABLE.clone(), subtest_name));

        let _table_test_case_name = variables.get(&VariableName::Custom(Cow::Borrowed(
            "_table_test_case_name",
        )));

        let go_table_test_case_variable = _table_test_case_name
            .and_then(extract_subtest_name)
            .map(|case_name| (GO_TABLE_TEST_CASE_NAME_TASK_VARIABLE.clone(), case_name));

        let _suite_name = variables.get(&VariableName::Custom(Cow::Borrowed("_suite_name")));

        let go_suite_variable = _suite_name
            .and_then(extract_subtest_name)
            .map(|suite_name| (GO_SUITE_NAME_TASK_VARIABLE.clone(), suite_name));

        Task::ready(Ok(TaskVariables::from_iter(
            [
                go_package_variable,
                go_subtest_variable,
                go_table_test_case_variable,
                go_suite_variable,
                go_module_root_variable,
            ]
            .into_iter()
            .flatten(),
        )))
    }

    fn associated_tasks(&self, _: Option<Entity<Buffer>>, _: &App) -> Task<Option<TaskTemplates>> {
        let package_cwd = if GO_PACKAGE_TASK_VARIABLE.template_value() == "." {
            None
        } else {
            Some("$ZED_DIRNAME".to_string())
        };
        let module_cwd = Some(GO_MODULE_ROOT_TASK_VARIABLE.template_value());

        Task::ready(Some(TaskTemplates(vec![
            TaskTemplate {
                label: format!(
                    "go test {} -v -run Test{}/{}",
                    GO_PACKAGE_TASK_VARIABLE.template_value(),
                    GO_SUITE_NAME_TASK_VARIABLE.template_value(),
                    VariableName::Symbol.template_value(),
                ),
                command: "go".into(),
                args: vec![
                    "test".into(),
                    "-v".into(),
                    "-run".into(),
                    format!(
                        "\\^Test{}\\$/\\^{}\\$",
                        GO_SUITE_NAME_TASK_VARIABLE.template_value(),
                        VariableName::Symbol.template_value(),
                    ),
                ],
                cwd: package_cwd.clone(),
                tags: vec!["go-testify-suite".to_owned()],
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!(
                    "go test {} -v -run {}/{}",
                    GO_PACKAGE_TASK_VARIABLE.template_value(),
                    VariableName::Symbol.template_value(),
                    GO_TABLE_TEST_CASE_NAME_TASK_VARIABLE.template_value(),
                ),
                command: "go".into(),
                args: vec![
                    "test".into(),
                    "-v".into(),
                    "-run".into(),
                    format!(
                        "\\^{}\\$/\\^{}\\$",
                        VariableName::Symbol.template_value(),
                        GO_TABLE_TEST_CASE_NAME_TASK_VARIABLE.template_value(),
                    ),
                ],
                cwd: package_cwd.clone(),
                tags: vec![
                    "go-table-test-case".to_owned(),
                    "go-table-test-case-without-explicit-variable".to_owned(),
                ],
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!(
                    "go test {} -run {}",
                    GO_PACKAGE_TASK_VARIABLE.template_value(),
                    VariableName::Symbol.template_value(),
                ),
                command: "go".into(),
                args: vec![
                    "test".into(),
                    "-run".into(),
                    format!("\\^{}\\$", VariableName::Symbol.template_value(),),
                ],
                tags: vec!["go-test".to_owned()],
                cwd: package_cwd.clone(),
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!(
                    "go test {} -run {}",
                    GO_PACKAGE_TASK_VARIABLE.template_value(),
                    VariableName::Symbol.template_value(),
                ),
                command: "go".into(),
                args: vec![
                    "test".into(),
                    "-run".into(),
                    format!("\\^{}\\$", VariableName::Symbol.template_value(),),
                ],
                tags: vec!["go-example".to_owned()],
                cwd: package_cwd.clone(),
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!("go test {}", GO_PACKAGE_TASK_VARIABLE.template_value()),
                command: "go".into(),
                args: vec!["test".into()],
                cwd: package_cwd.clone(),
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: "go test ./...".into(),
                command: "go".into(),
                args: vec!["test".into(), "./...".into()],
                cwd: module_cwd.clone(),
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!(
                    "go test {} -v -run {}/{}",
                    GO_PACKAGE_TASK_VARIABLE.template_value(),
                    VariableName::Symbol.template_value(),
                    GO_SUBTEST_NAME_TASK_VARIABLE.template_value(),
                ),
                command: "go".into(),
                args: vec![
                    "test".into(),
                    "-v".into(),
                    "-run".into(),
                    format!(
                        "\\^{}\\$/\\^{}\\$",
                        VariableName::Symbol.template_value(),
                        GO_SUBTEST_NAME_TASK_VARIABLE.template_value(),
                    ),
                ],
                cwd: package_cwd.clone(),
                tags: vec!["go-subtest".to_owned()],
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!(
                    "go test {} -bench {}",
                    GO_PACKAGE_TASK_VARIABLE.template_value(),
                    VariableName::Symbol.template_value()
                ),
                command: "go".into(),
                args: vec![
                    "test".into(),
                    "-benchmem".into(),
                    "-run='^$'".into(),
                    "-bench".into(),
                    format!("\\^{}\\$", VariableName::Symbol.template_value()),
                ],
                cwd: package_cwd.clone(),
                tags: vec!["go-benchmark".to_owned()],
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!(
                    "go test {} -fuzz=Fuzz -run {}",
                    GO_PACKAGE_TASK_VARIABLE.template_value(),
                    VariableName::Symbol.template_value(),
                ),
                command: "go".into(),
                args: vec![
                    "test".into(),
                    "-fuzz=Fuzz".into(),
                    "-run".into(),
                    format!("\\^{}\\$", VariableName::Symbol.template_value(),),
                ],
                tags: vec!["go-fuzz".to_owned()],
                cwd: package_cwd.clone(),
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!("go run {}", GO_PACKAGE_TASK_VARIABLE.template_value(),),
                command: "go".into(),
                args: vec!["run".into(), ".".into()],
                cwd: package_cwd.clone(),
                tags: vec!["go-main".to_owned()],
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!("go generate {}", GO_PACKAGE_TASK_VARIABLE.template_value()),
                command: "go".into(),
                args: vec!["generate".into()],
                cwd: package_cwd,
                tags: vec!["go-generate".to_owned()],
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: "go generate ./...".into(),
                command: "go".into(),
                args: vec!["generate".into(), "./...".into()],
                cwd: module_cwd,
                ..TaskTemplate::default()
            },
        ])))
    }

    fn runnable_resolver(&self) -> Option<Arc<dyn RunnableResolver>> {
        Some(Arc::new(GoRunnableResolver))
    }
}

fn extract_subtest_name(input: &str) -> Option<String> {
    let content = if input.starts_with('`') && input.ends_with('`') {
        input.trim_matches('`')
    } else {
        input.trim_matches('"')
    };

    let processed = content
        .chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect::<String>();

    Some(
        GO_ESCAPE_SUBTEST_NAME_REGEX
            .replace_all(&processed, |caps: &regex::Captures| {
                format!("\\{}", &caps[0])
            })
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language;
    use gpui::{AppContext, TestAppContext};
    use task::TaskContext;
    
    use unindent::Unindent as _;

    fn go_language() -> Arc<Language> {
        let language = language("go", tree_sitter_go::LANGUAGE.into());
        Arc::new(
            Arc::try_unwrap(language)
                .unwrap()
                .with_context_provider(Some(Arc::new(GoContextProvider))),
        )
    }

    #[gpui::test]
    fn test_go_test_main_ignored(cx: &mut TestAppContext) {
        let language = go_language();

        let example_test = r#"
        package main

        func TestMain(m *testing.M) {
            os.Exit(m.Run())
        }
        "#;

        let buffer =
            cx.new(|cx| crate::Buffer::local(example_test, cx).with_language(language.clone(), cx));
        cx.executor().run_until_parked();

        let runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot.runnable_ranges(0..example_test.len()).collect()
        });

        let tag_strings: Vec<String> = runnables
            .iter()
            .flat_map(|r| &r.runnable.tags)
            .map(|tag| tag.0.to_string())
            .collect();

        assert!(
            !tag_strings.contains(&"go-test".to_string()),
            "Should NOT find go-test tag, found: {:?}",
            tag_strings
        );
    }

    #[gpui::test]
    fn test_testify_suite_detection(cx: &mut TestAppContext) {
        let language = go_language();

        let testify_suite = r#"
        package main

        import (
            "testing"

            "github.com/stretchr/testify/suite"
        )

        type ExampleSuite struct {
            suite.Suite
        }

        func TestExampleSuite(t *testing.T) {
            suite.Run(t, new(ExampleSuite))
        }

        func (s *ExampleSuite) TestSomething_Success() {
            // test code
        }
        "#;

        let buffer = cx
            .new(|cx| crate::Buffer::local(testify_suite, cx).with_language(language.clone(), cx));
        cx.executor().run_until_parked();

        let runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot.runnable_ranges(0..testify_suite.len()).collect()
        });

        let tag_strings: Vec<String> = runnables
            .iter()
            .flat_map(|r| &r.runnable.tags)
            .map(|tag| tag.0.to_string())
            .collect();

        assert!(
            tag_strings.contains(&"go-test".to_string()),
            "Should find go-test tag, found: {:?}",
            tag_strings
        );
        assert!(
            tag_strings.contains(&"go-testify-suite".to_string()),
            "Should find go-testify-suite tag, found: {:?}",
            tag_strings
        );
    }

    #[gpui::test]
    fn test_go_runnable_detection(cx: &mut TestAppContext) {
        let language = go_language();

        let interpreted_string_subtest = r#"
        package main

        import "testing"

        func TestExample(t *testing.T) {
            t.Run("subtest with double quotes", func(t *testing.T) {
                // test code
            })
        }
        "#;

        let raw_string_subtest = r#"
        package main

        import "testing"

        func TestExample(t *testing.T) {
            t.Run(`subtest with
            multiline
            backticks`, func(t *testing.T) {
                // test code
            })
        }
        "#;

        let buffer = cx.new(|cx| {
            crate::Buffer::local(interpreted_string_subtest, cx).with_language(language.clone(), cx)
        });
        cx.executor().run_until_parked();

        let runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot
                .runnable_ranges(0..interpreted_string_subtest.len())
                .collect()
        });

        let tag_strings: Vec<String> = runnables
            .iter()
            .flat_map(|r| &r.runnable.tags)
            .map(|tag| tag.0.to_string())
            .collect();

        assert!(
            tag_strings.contains(&"go-test".to_string()),
            "Should find go-test tag, found: {:?}",
            tag_strings
        );
        assert!(
            tag_strings.contains(&"go-subtest".to_string()),
            "Should find go-subtest tag, found: {:?}",
            tag_strings
        );

        let buffer = cx.new(|cx| {
            crate::Buffer::local(raw_string_subtest, cx).with_language(language.clone(), cx)
        });
        cx.executor().run_until_parked();

        let runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot
                .runnable_ranges(0..raw_string_subtest.len())
                .collect()
        });

        let tag_strings: Vec<String> = runnables
            .iter()
            .flat_map(|r| &r.runnable.tags)
            .map(|tag| tag.0.to_string())
            .collect();

        assert!(
            tag_strings.contains(&"go-test".to_string()),
            "Should find go-test tag, found: {:?}",
            tag_strings
        );
        assert!(
            tag_strings.contains(&"go-subtest".to_string()),
            "Should find go-subtest tag, found: {:?}",
            tag_strings
        );
    }

    #[gpui::test]
    async fn test_go_test_templates_run_arg_is_shell_escaped(cx: &mut TestAppContext) {
        let templates = cx
            .update(|cx| GoContextProvider.associated_tasks(None, cx))
            .await
            .expect("Go context provider returns associated tasks");

        // `resolve_task` returns `None` for any `ZED_` variable a template
        // references but the context omits, so supply all of them.
        let context = TaskContext {
            cwd: None,
            task_variables: TaskVariables::from_iter([
                (VariableName::Symbol, "TestFoo".to_string()),
                (GO_SUBTEST_NAME_TASK_VARIABLE, "simple_subtest".to_string()),
                (
                    GO_TABLE_TEST_CASE_NAME_TASK_VARIABLE,
                    "table_case".to_string(),
                ),
                (GO_SUITE_NAME_TASK_VARIABLE, "Suite".to_string()),
                (GO_PACKAGE_TASK_VARIABLE, ".".to_string()),
                (VariableName::Dirname, "/tmp".to_string()),
            ]),
            project_env: HashMap::default(),
        };

        // `go-benchmark` is excluded: its `-run='^$'` form intentionally quotes.
        let escaped_run_arg_tags = [
            "go-test",
            "go-example",
            "go-subtest",
            "go-fuzz",
            "go-testify-suite",
            "go-table-test-case",
        ];

        for tag in escaped_run_arg_tags {
            let template = templates
                .0
                .iter()
                .find(|template| template.tags.iter().any(|template_tag| template_tag == tag))
                .unwrap_or_else(|| panic!("`{tag}` task template exists"));

            let resolved = template
                .resolve_task("go", &context)
                .unwrap_or_else(|| panic!("`{tag}` template resolves"));

            let run_index = resolved
                .resolved
                .args
                .iter()
                .position(|arg| arg == "-run")
                .unwrap_or_else(|| panic!("`{tag}` resolved args contain a `-run` flag"));
            let run_arg = &resolved.resolved.args[run_index + 1];

            assert!(
                !run_arg.contains('\''),
                "`{tag}` -run arg must not shell-quote the regex; single quotes leak \
                 literally into Delve's regex and break Debug Test (#53230), got {run_arg:?}"
            );
            assert!(
                run_arg.starts_with("\\^") && run_arg.ends_with("\\$"),
                "`{tag}` -run arg must escape its regex anchors as \\^...\\$ so the shell \
                 and GoLocator both strip them, got {run_arg:?}"
            );
        }
    }

    #[gpui::test]
    fn test_go_example_test_detection(cx: &mut TestAppContext) {
        let language = go_language();

        let example_test = r#"
        package main

        import "fmt"

        func Example() {
            fmt.Println("Hello, world!")
            // Output: Hello, world!
        }
        "#;

        let buffer =
            cx.new(|cx| crate::Buffer::local(example_test, cx).with_language(language.clone(), cx));
        cx.executor().run_until_parked();

        let runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot.runnable_ranges(0..example_test.len()).collect()
        });

        let tag_strings: Vec<String> = runnables
            .iter()
            .flat_map(|r| &r.runnable.tags)
            .map(|tag| tag.0.to_string())
            .collect();

        assert!(
            tag_strings.contains(&"go-example".to_string()),
            "Should find go-example tag, found: {:?}",
            tag_strings
        );
    }

    #[gpui::test]
    fn test_go_table_test_slice_detection(cx: &mut TestAppContext) {
        let language = go_language();

        let table_test = r#"
        package main

        import "testing"

        func TestExample(t *testing.T) {
            _ = "some random string"

            testCases := []struct{
                name string
                anotherStr string
            }{
                {
                    name: "test case 1",
                    anotherStr: "foo",
                },
                {
                    name: "test case 2",
                    anotherStr: "bar",
                },
                {
                    name: "test case 3",
                    anotherStr: "baz",
                },
            }

            notATableTest := []struct{
                name string
            }{
                {
                    name: "some string",
                },
                {
                    name: "some other string",
                },
            }

            for _, tc := range testCases {
                t.Run(tc.name, func(t *testing.T) {
                    // test code here
                })
            }
        }
        "#;

        let buffer =
            cx.new(|cx| crate::Buffer::local(table_test, cx).with_language(language.clone(), cx));
        cx.executor().run_until_parked();

        let runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot.runnable_ranges(0..table_test.len()).collect()
        });

        let tag_strings: Vec<String> = runnables
            .iter()
            .flat_map(|r| &r.runnable.tags)
            .map(|tag| tag.0.to_string())
            .collect();

        assert!(
            tag_strings.contains(&"go-test".to_string()),
            "Should find go-test tag, found: {:?}",
            tag_strings
        );
        assert!(
            tag_strings.contains(&"go-table-test-case".to_string()),
            "Should find go-table-test-case tag, found: {:?}",
            tag_strings
        );

        let go_test_count = tag_strings.iter().filter(|&tag| tag == "go-test").count();
        let go_table_test_count = tag_strings
            .iter()
            .filter(|&tag| tag == "go-table-test-case")
            .count();

        assert!(
            go_test_count == 1,
            "Should find exactly 1 go-test, found: {}",
            go_test_count
        );
        assert!(
            go_table_test_count == 3,
            "Should find exactly 3 go-table-test-case, found: {}",
            go_table_test_count
        );

        let Some(first_case_offset) = table_test.find("anotherStr: \"foo\"") else {
            panic!("missing first table test case");
        };
        let first_case_offset = first_case_offset + "anotherStr".len();
        let first_case_runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot
                .runnable_ranges(first_case_offset..first_case_offset)
                .collect()
        });
        let table_test_case_names: Vec<_> = first_case_runnables
            .iter()
            .filter(|runnable| {
                runnable
                    .runnable
                    .tags
                    .iter()
                    .any(|tag| tag.0 == "go-table-test-case")
            })
            .filter_map(|runnable| runnable.extra_captures.get("_table_test_case_name"))
            .collect();

        assert_eq!(
            table_test_case_names,
            vec!["\"test case 1\""],
            "Should only return the table test case containing the requested range"
        );
    }

    #[gpui::test]
    fn test_go_table_test_slice_without_explicit_variable_detection(cx: &mut TestAppContext) {
        let language = go_language();

        let table_test = r#"
        package main

        import "testing"

        func TestExample(t *testing.T) {
            for _, tc := range []struct{
                name string
                anotherStr string
            }{
                {
                    name: "test case 1",
                    anotherStr: "foo",
                },
                {
                    name: "test case 2",
                    anotherStr: "bar",
                },
                {
                    name: "test case 3",
                    anotherStr: "baz",
                },
            } {
                t.Run(tc.name, func(t *testing.T) {
                    // test code here
                })
            }
        }
        "#;

        let buffer =
            cx.new(|cx| crate::Buffer::local(table_test, cx).with_language(language.clone(), cx));
        cx.executor().run_until_parked();

        let runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot.runnable_ranges(0..table_test.len()).collect()
        });

        let tag_strings: Vec<String> = runnables
            .iter()
            .flat_map(|r| &r.runnable.tags)
            .map(|tag| tag.0.to_string())
            .collect();

        assert!(
            tag_strings.contains(&"go-test".to_string()),
            "Should find go-test tag, found: {:?}",
            tag_strings
        );
        assert!(
            tag_strings.contains(&"go-table-test-case-without-explicit-variable".to_string()),
            "Should find go-table-test-case-without-explicit-variable tag, found: {:?}",
            tag_strings
        );

        let go_test_count = tag_strings.iter().filter(|&tag| tag == "go-test").count();
        let go_table_test_count = tag_strings
            .iter()
            .filter(|&tag| tag == "go-table-test-case-without-explicit-variable")
            .count();

        assert!(
            go_test_count == 1,
            "Should find exactly 1 go-test, found: {}",
            go_test_count
        );
        assert!(
            go_table_test_count == 3,
            "Should find exactly 3 go-table-test-case-without-explicit-variable, found: {}",
            go_table_test_count
        );
    }

    #[gpui::test]
    fn test_go_table_test_map_without_explicit_variable_detection(cx: &mut TestAppContext) {
        let language = go_language();

        let table_test = r#"
        package main

        import "testing"

        func TestExample(t *testing.T) {
            for name, tc := range map[string]struct {
          		someStr string
          		fail    bool
           	}{
          		"test failure": {
         			someStr: "foo",
         			fail:    true,
          		},
          		"test success": {
         			someStr: "bar",
         			fail:    false,
          		},
           	} {
                t.Run(name, func(t *testing.T) {
                    // test code here
                })
            }
        }
        "#;

        let buffer =
            cx.new(|cx| crate::Buffer::local(table_test, cx).with_language(language.clone(), cx));
        cx.executor().run_until_parked();

        let runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot.runnable_ranges(0..table_test.len()).collect()
        });

        let tag_strings: Vec<String> = runnables
            .iter()
            .flat_map(|r| &r.runnable.tags)
            .map(|tag| tag.0.to_string())
            .collect();

        assert!(
            tag_strings.contains(&"go-test".to_string()),
            "Should find go-test tag, found: {:?}",
            tag_strings
        );
        assert!(
            tag_strings.contains(&"go-table-test-case-without-explicit-variable".to_string()),
            "Should find go-table-test-case-without-explicit-variable tag, found: {:?}",
            tag_strings
        );

        let go_test_count = tag_strings.iter().filter(|&tag| tag == "go-test").count();
        let go_table_test_count = tag_strings
            .iter()
            .filter(|&tag| tag == "go-table-test-case-without-explicit-variable")
            .count();

        assert!(
            go_test_count == 1,
            "Should find exactly 1 go-test, found: {}",
            go_test_count
        );
        assert!(
            go_table_test_count == 2,
            "Should find exactly 2 go-table-test-case-without-explicit-variable, found: {}",
            go_table_test_count
        );
    }

    #[gpui::test]
    fn test_go_table_test_slice_ignored(cx: &mut TestAppContext) {
        let language = go_language();

        let table_test = r#"
        package main

        func Example() {
            _ = "some random string"

            notATableTest := []struct{
                name string
            }{
                {
                    name: "some string",
                },
                {
                    name: "some other string",
                },
            }
        }
        "#;

        let buffer =
            cx.new(|cx| crate::Buffer::local(table_test, cx).with_language(language.clone(), cx));
        cx.executor().run_until_parked();

        let runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot.runnable_ranges(0..table_test.len()).collect()
        });

        let tag_strings: Vec<String> = runnables
            .iter()
            .flat_map(|r| &r.runnable.tags)
            .map(|tag| tag.0.to_string())
            .collect();

        assert!(
            !tag_strings.contains(&"go-test".to_string()),
            "Should find go-test tag, found: {:?}",
            tag_strings
        );
        assert!(
            !tag_strings.contains(&"go-table-test-case".to_string()),
            "Should find go-table-test-case tag, found: {:?}",
            tag_strings
        );
    }

    #[gpui::test]
    fn test_go_table_test_map_detection(cx: &mut TestAppContext) {
        let language = go_language();

        let table_test = r#"
        package main

        import "testing"

        func TestExample(t *testing.T) {
            _ = "some random string"

           	testCases := map[string]struct {
          		someStr string
          		fail    bool
           	}{
          		"test failure": {
         			someStr: "foo",
         			fail:    true,
          		},
          		"test success": {
         			someStr: "bar",
         			fail:    false,
          		},
           	}

           	notATableTest := map[string]struct {
          		someStr string
           	}{
          		"some string": {
         			someStr: "foo",
          		},
          		"some other string": {
         			someStr: "bar",
          		},
           	}

            for name, tc := range testCases {
                t.Run(name, func(t *testing.T) {
                    // test code here
                })
            }
        }
        "#;

        let buffer =
            cx.new(|cx| crate::Buffer::local(table_test, cx).with_language(language.clone(), cx));
        cx.executor().run_until_parked();

        let runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot.runnable_ranges(0..table_test.len()).collect()
        });

        let tag_strings: Vec<String> = runnables
            .iter()
            .flat_map(|r| &r.runnable.tags)
            .map(|tag| tag.0.to_string())
            .collect();

        assert!(
            tag_strings.contains(&"go-test".to_string()),
            "Should find go-test tag, found: {:?}",
            tag_strings
        );
        assert!(
            tag_strings.contains(&"go-table-test-case".to_string()),
            "Should find go-table-test-case tag, found: {:?}",
            tag_strings
        );

        let go_test_count = tag_strings.iter().filter(|&tag| tag == "go-test").count();
        let go_table_test_count = tag_strings
            .iter()
            .filter(|&tag| tag == "go-table-test-case")
            .count();

        assert!(
            go_test_count == 1,
            "Should find exactly 1 go-test, found: {}",
            go_test_count
        );
        assert!(
            go_table_test_count == 2,
            "Should find exactly 2 go-table-test-case, found: {}",
            go_table_test_count
        );
    }

    #[gpui::test]
    fn test_go_table_test_map_ignored(cx: &mut TestAppContext) {
        let language = go_language();

        let table_test = r#"
        package main

        func Example() {
            _ = "some random string"

           	notATableTest := map[string]struct {
          		someStr string
           	}{
          		"some string": {
         			someStr: "foo",
          		},
          		"some other string": {
         			someStr: "bar",
          		},
           	}
        }
        "#;

        let buffer =
            cx.new(|cx| crate::Buffer::local(table_test, cx).with_language(language.clone(), cx));
        cx.executor().run_until_parked();

        let runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot.runnable_ranges(0..table_test.len()).collect()
        });

        let tag_strings: Vec<String> = runnables
            .iter()
            .flat_map(|r| &r.runnable.tags)
            .map(|tag| tag.0.to_string())
            .collect();

        assert!(
            !tag_strings.contains(&"go-test".to_string()),
            "Should find go-test tag, found: {:?}",
            tag_strings
        );
        assert!(
            !tag_strings.contains(&"go-table-test-case".to_string()),
            "Should find go-table-test-case tag, found: {:?}",
            tag_strings
        );
    }

    #[gpui::test]
    fn test_go_table_test_stress(cx: &mut TestAppContext) {
        let language = go_language();

        let mut entries = String::new();
        for i in 0..100 {
            entries.push_str(&format!(
                "                {{ name: \"case {}\", value: {} }},\n",
                i, i
            ));
        }
        let table_test = format!(
            r#"
        package main

        import "testing"

        func TestStress(t *testing.T) {{
            testCases := []struct{{
                name  string
                value int
            }}{{
{entries}            }}

            for _, tc := range testCases {{
                t.Run(tc.name, func(t *testing.T) {{
                    _ = tc.value
                }})
            }}
        }}
        "#,
            entries = entries
        );

        let buffer = cx.new(|cx| {
            crate::Buffer::local(table_test.clone(), cx).with_language(language.clone(), cx)
        });
        cx.executor().run_until_parked();

        let runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot.runnable_ranges(0..table_test.len()).collect()
        });

        let tag_strings: Vec<String> = runnables
            .iter()
            .flat_map(|r| &r.runnable.tags)
            .map(|tag| tag.0.to_string())
            .collect();

        let go_table_test_count = tag_strings
            .iter()
            .filter(|&tag| tag == "go-table-test-case")
            .count();

        assert_eq!(
            go_table_test_count, 100,
            "Should emit one go-table-test-case per row (got {}); tree-sitter match_limit overflow has regressed",
            go_table_test_count
        );
    }

    #[gpui::test]
    fn test_go_table_test_mismatched_field(cx: &mut TestAppContext) {
        let language = go_language();

        let table_test = r#"
        package main

        import "testing"

        func TestMismatchedField(t *testing.T) {
            testCases := []struct{
                name string
            }{
                { name: "test case 1" },
                { name: "test case 2" },
            }

            for _, tc := range testCases {
                t.Run(tc.desc, func(t *testing.T) {
                    // test code here
                })
            }
        }
        "#;

        let buffer =
            cx.new(|cx| crate::Buffer::local(table_test, cx).with_language(language.clone(), cx));
        cx.executor().run_until_parked();

        let runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot.runnable_ranges(0..table_test.len()).collect()
        });

        let tag_strings: Vec<String> = runnables
            .iter()
            .flat_map(|r| &r.runnable.tags)
            .map(|tag| tag.0.to_string())
            .collect();

        let go_table_test_count = tag_strings
            .iter()
            .filter(|&tag| tag == "go-table-test-case")
            .count();

        assert_eq!(
            go_table_test_count, 0,
            "Should not emit table-test runnables when t.Run uses a missing row field"
        );
    }

    #[gpui::test]
    fn test_go_table_test_slice_picks_correct_field_when_not_first(cx: &mut TestAppContext) {
        // The subtest-name field `name` is declared AFTER `anotherStr`, but `t.Run(tc.name, ...)`
        // still selects on `name`. The resolver must match `@_field_check` text to the right
        // `@_field_name` regardless of source order; "first string field wins" would be a bug.
        let language = go_language();

        let table_test = r#"
        package main

        import "testing"

        func TestExample(t *testing.T) {
            testCases := []struct{
                anotherStr string
                name       string
            }{
                {
                    anotherStr: "alpha",
                    name:       "case alpha",
                },
                {
                    anotherStr: "beta",
                    name:       "case beta",
                },
            }

            for _, tc := range testCases {
                t.Run(tc.name, func(t *testing.T) {
                    _ = tc.anotherStr
                })
            }
        }
        "#;

        let buffer =
            cx.new(|cx| crate::Buffer::local(table_test, cx).with_language(language.clone(), cx));
        cx.executor().run_until_parked();

        let case_offset = table_test
            .find("anotherStr: \"alpha\"")
            .expect("source should contain the first case body");
        let first_case_runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot.runnable_ranges(case_offset..case_offset).collect()
        });

        let case_names: Vec<_> = first_case_runnables
            .iter()
            .filter(|runnable| {
                runnable
                    .runnable
                    .tags
                    .iter()
                    .any(|tag| tag.0 == "go-table-test-case")
            })
            .filter_map(|runnable| runnable.extra_captures.get("_table_test_case_name"))
            .collect();

        assert_eq!(
            case_names,
            vec!["\"case alpha\""],
            "Resolver should pick the field matching `tc.name`, not the first string field"
        );
    }

    #[gpui::test]
    fn test_go_table_test_map_extras_include_case_name(cx: &mut TestAppContext) {
        let language = go_language();

        let table_test = r#"
        package main

        import "testing"

        func TestExample(t *testing.T) {
            testCases := map[string]struct {
                fail bool
            }{
                "test failure": {fail: true},
                "test success": {fail: false},
            }

            for name, tc := range testCases {
                t.Run(name, func(t *testing.T) {
                    _ = tc.fail
                })
            }
        }
        "#;

        let buffer =
            cx.new(|cx| crate::Buffer::local(table_test, cx).with_language(language.clone(), cx));
        cx.executor().run_until_parked();

        let all_runnables: Vec<_> = buffer.update(cx, |buffer, _| {
            let snapshot = buffer.snapshot();
            snapshot.runnable_ranges(0..table_test.len()).collect()
        });
        let all_case_names: Vec<_> = all_runnables
            .iter()
            .filter(|runnable| {
                runnable
                    .runnable
                    .tags
                    .iter()
                    .any(|tag| tag.0 == "go-table-test-case")
            })
            .filter_map(|runnable| runnable.extra_captures.get("_table_test_case_name"))
            .cloned()
            .collect();
        assert_eq!(
            all_case_names,
            vec!["\"test failure\"", "\"test success\""],
            "Map-based table tests should surface each row's key as `_table_test_case_name`"
        );
    }

    #[gpui::test]
    fn test_go_outline_includes_methods_with_receiver_forms(cx: &mut TestAppContext) {
        let language = go_language();

        let source = r#"
        package main

        type v2 struct{}

        func (v2) BrokenMethod() {
            println("start")
        }

        func (_ v2) UnderscoreReceiverMethod() {
            println("start")
        }

        func (v v2) NamedReceiverMethod() {
            println("start")
        }

        func (v *v2) PointerReceiverMethod() {
            println("start")
        }

        func WorkingFunction() {
            println("start")
        }
        "#
        .unindent();

        let buffer =
            cx.new(|cx| crate::Buffer::local(source.clone(), cx).with_language(language, cx));
        let snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());
        let outline = snapshot.outline(None);

        assert_eq!(
            outline
                .items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>(),
            &[
                "type v2",
                "func (v2) BrokenMethod",
                "func (_ v2) UnderscoreReceiverMethod",
                "func (v v2) NamedReceiverMethod",
                "func (v *v2) PointerReceiverMethod",
                "func WorkingFunction",
            ]
        );

        for (method_name, expected_symbol) in [
            ("BrokenMethod", "func (v2) BrokenMethod"),
            (
                "UnderscoreReceiverMethod",
                "func (_ v2) UnderscoreReceiverMethod",
            ),
            ("NamedReceiverMethod", "func (v v2) NamedReceiverMethod"),
            (
                "PointerReceiverMethod",
                "func (v *v2) PointerReceiverMethod",
            ),
            ("WorkingFunction", "func WorkingFunction"),
        ] {
            let method_position = source
                .find(&format!("{method_name}()"))
                .expect("method should exist in source");
            let body_position = source[method_position..]
                .find("println")
                .map(|body_offset| method_position + body_offset)
                .expect("method should contain a body");
            let symbols = snapshot.symbols_containing(body_position, None);
            assert_eq!(
                symbols
                    .last()
                    .map(|item| item.text.as_str())
                    .expect("method should have an outline symbol"),
                expected_symbol
            );
        }
    }

    #[test]
    fn test_extract_subtest_name() {
        // Interpreted string literal
        let input_double_quoted = r#""subtest with double quotes""#;
        let result = extract_subtest_name(input_double_quoted);
        assert_eq!(result, Some(r#"subtest_with_double_quotes"#.to_string()));

        let input_double_quoted_with_backticks = r#""test with `backticks` inside""#;
        let result = extract_subtest_name(input_double_quoted_with_backticks);
        assert_eq!(result, Some(r#"test_with_`backticks`_inside"#.to_string()));

        // Raw string literal
        let input_with_backticks = r#"`subtest with backticks`"#;
        let result = extract_subtest_name(input_with_backticks);
        assert_eq!(result, Some(r#"subtest_with_backticks"#.to_string()));

        let input_raw_with_quotes = r#"`test with "quotes" and other chars`"#;
        let result = extract_subtest_name(input_raw_with_quotes);
        assert_eq!(
            result,
            Some(r#"test_with_\"quotes\"_and_other_chars"#.to_string())
        );

        let input_multiline = r#"`subtest with
        multiline
        backticks`"#;
        let result = extract_subtest_name(input_multiline);
        assert_eq!(
            result,
            Some(r#"subtest_with_________multiline_________backticks"#.to_string())
        );

        let input_with_double_quotes = r#"`test with "double quotes"`"#;
        let result = extract_subtest_name(input_with_double_quotes);
        assert_eq!(result, Some(r#"test_with_\"double_quotes\""#.to_string()));
    }
}
