pub use language::*;

use std::sync::Arc;

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

impl ContextProvider for GoContextProvider {
    fn runnable_resolver(&self) -> Option<Arc<dyn RunnableResolver>> {
        Some(Arc::new(GoRunnableResolver))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language;
    use gpui::{AppContext, TestAppContext};

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
}
