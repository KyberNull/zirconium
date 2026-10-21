#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, Hsla, TestAppContext};
    use rope::Rope;
    use theme::SyntaxTheme;
    use unindent::Unindent;

    #[test]
    fn test_class_instantiation_highlighting() {
        let source = Rope::from("class Dog {}\nconst dog = new Dog();");
        let theme = SyntaxTheme::new_test([("type", Hsla::blue()), ("type.class", Hsla::green())]);

        for language in [
            crate::language(
                "typescript",
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            ),
            crate::language("tsx", tree_sitter_typescript::LANGUAGE_TSX.into()),
            crate::language("javascript", tree_sitter_typescript::LANGUAGE_TSX.into()),
        ] {
            language.set_theme(&theme);
            let class_highlight = language
                .grammar()
                .and_then(|grammar| grammar.highlight_id_for_name("type.class"))
                .expect("type.class highlight should be defined");

            assert_eq!(
                language.highlight_text(&source, 0..source.len()),
                vec![(6..9, class_highlight), (29..32, class_highlight)],
                "{} class instantiations should use the type.class highlight",
                language.name()
            );
        }
    }

    #[gpui::test]
    async fn test_outline(cx: &mut TestAppContext) {
        for language in [
            crate::language(
                "typescript",
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            ),
            crate::language("tsx", tree_sitter_typescript::LANGUAGE_TSX.into()),
        ] {
            let text = r#"
            function a() {
              // local variables are included
              let a1 = 1;
              // all functions are included
              async function a2() {}
            }
            // top-level variables are included
            let b: C
            function getB() {}
            // exported variables are included
            export const d = e;
        "#
            .unindent();

            let buffer = cx.new(|cx| language::Buffer::local(text, cx).with_language(language, cx));
            let outline = buffer.read_with(cx, |buffer, _| buffer.snapshot().outline(None));
            assert_eq!(
                outline
                    .items
                    .iter()
                    .map(|item| (item.text.as_str(), item.depth))
                    .collect::<Vec<_>>(),
                &[
                    ("function a()", 0),
                    ("let a1", 1),
                    ("async function a2()", 1),
                    ("let b", 0),
                    ("function getB()", 0),
                    ("const d", 0),
                ]
            );
        }
    }

    #[gpui::test]
    async fn test_outline_with_destructuring(cx: &mut TestAppContext) {
        for language in [
            crate::language(
                "typescript",
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            ),
            crate::language("tsx", tree_sitter_typescript::LANGUAGE_TSX.into()),
        ] {
            let text = r#"
            // Top-level destructuring
            const { a1, a2 } = a;
            const [b1, b2] = b;

            // Defaults and rest
            const [c1 = 1, , c2, ...rest1] = c;
            const { d1, d2: e1, f1 = 2, g1: h1 = 3, ...rest2 } = d;

            function processData() {
              // Nested object destructuring
              const { c1, c2 } = c;
              // Nested array destructuring
              const [d1, d2, d3] = d;
              // Destructuring with renaming
              const { f1: g1 } = f;
              // With defaults
              const [x = 10, y] = xy;
            }

            class DataHandler {
              method() {
                // Destructuring in class method
                const { a1, a2 } = a;
                const [b1, ...b2] = b;
              }
            }
        "#
            .unindent();

            let buffer = cx.new(|cx| language::Buffer::local(text, cx).with_language(language, cx));
            let outline = buffer.read_with(cx, |buffer, _| buffer.snapshot().outline(None));
            assert_eq!(
                outline
                    .items
                    .iter()
                    .map(|item| (item.text.as_str(), item.depth))
                    .collect::<Vec<_>>(),
                &[
                    ("const a1", 0),
                    ("const a2", 0),
                    ("const b1", 0),
                    ("const b2", 0),
                    ("const c1", 0),
                    ("const c2", 0),
                    ("const rest1", 0),
                    ("const d1", 0),
                    ("const e1", 0),
                    ("const h1", 0),
                    ("const rest2", 0),
                    ("function processData()", 0),
                    ("const c1", 1),
                    ("const c2", 1),
                    ("const d1", 1),
                    ("const d2", 1),
                    ("const d3", 1),
                    ("const g1", 1),
                    ("const x", 1),
                    ("const y", 1),
                    ("class DataHandler", 0),
                    ("method()", 1),
                    ("const a1", 2),
                    ("const a2", 2),
                    ("const b1", 2),
                    ("const b2", 2),
                ]
            );
        }
    }

    #[gpui::test]
    async fn test_outline_with_object_properties(cx: &mut TestAppContext) {
        for language in [
            crate::language(
                "typescript",
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            ),
            crate::language("tsx", tree_sitter_typescript::LANGUAGE_TSX.into()),
        ] {
            let text = r#"
            // Object with function properties
            const o = { m() {}, async n() {}, g: function* () {}, h: () => {}, k: function () {} };

            // Object with primitive properties
            const p = { p1: 1, p2: "hello", p3: true };

            // Nested objects
            const q = {
                r: {
                    // won't be included due to one-level depth limit
                    s: 1
                },
                t: 2
            };

            function getData() {
                const local = { x: 1, y: 2 };
                return local;
            }
        "#
            .unindent();

            let buffer = cx.new(|cx| language::Buffer::local(text, cx).with_language(language, cx));
            cx.run_until_parked();
            let outline = buffer.read_with(cx, |buffer, _| buffer.snapshot().outline(None));
            assert_eq!(
                outline
                    .items
                    .iter()
                    .map(|item| (item.text.as_str(), item.depth))
                    .collect::<Vec<_>>(),
                &[
                    ("const o", 0),
                    ("m()", 1),
                    ("async n()", 1),
                    ("g", 1),
                    ("h", 1),
                    ("k", 1),
                    ("const p", 0),
                    ("p1", 1),
                    ("p2", 1),
                    ("p3", 1),
                    ("const q", 0),
                    ("r", 1),
                    ("s", 2),
                    ("t", 1),
                    ("function getData()", 0),
                    ("const local", 1),
                    ("x", 2),
                    ("y", 2),
                ]
            );
        }
    }

    #[gpui::test]
    async fn test_outline_with_nested_object_methods(cx: &mut TestAppContext) {
        for language in [
            crate::language(
                "typescript",
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            ),
            crate::language("tsx", tree_sitter_typescript::LANGUAGE_TSX.into()),
            crate::language("javascript", tree_sitter_typescript::LANGUAGE_TSX.into()),
        ] {
            let text = r#"
            // Reproduction from https://github.com/zed-industries/zed/issues/48711
            const a = {
              p01: '01',
              fn01: () => {},
              fn02() {},
              deep: {
                subFn01: () => {},
                subFn02() {},
                subP03: '03',
                deep2: {
                  subFn01: () => {},
                  subFn02() {},
                  subP03: '03',
                },
              },
            };

            // Edge case: async methods in nested objects
            const b = {
              async topAsync() {},
              nested: { async nestedAsync() {} },
            };

            // Edge case: object literal in function argument
            foo({ bar() {}, inner: { baz() {} } });
        "#
            .unindent();

            let buffer = cx.new(|cx| language::Buffer::local(text, cx).with_language(language, cx));
            cx.run_until_parked();
            let outline = buffer.read_with(cx, |buffer, _| buffer.snapshot().outline(None));

            let items: Vec<_> = outline
                .items
                .iter()
                .map(|item| (item.text.as_str(), item.depth))
                .collect();

            assert_eq!(
                items,
                &[
                    ("const a", 0),
                    ("p01", 1),
                    ("fn01", 1),
                    ("fn02()", 1),
                    ("deep", 1),
                    ("subFn01", 2),
                    ("subFn02()", 2),
                    ("subP03", 2),
                    ("deep2", 2),
                    ("subFn01", 3),
                    ("subFn02()", 3),
                    ("subP03", 3),
                    ("const b", 0),
                    ("async topAsync()", 1),
                    ("nested", 1),
                    ("async nestedAsync()", 2),
                    ("bar()", 0),
                    ("inner", 0),
                    ("baz()", 1),
                ]
            );
        }
    }

    #[gpui::test]
    async fn test_outline_with_complex_nested_objects(cx: &mut TestAppContext) {
        for language in [
            crate::language(
                "typescript",
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            ),
            crate::language("tsx", tree_sitter_typescript::LANGUAGE_TSX.into()),
            crate::language("javascript", tree_sitter_typescript::LANGUAGE_TSX.into()),
        ] {
            let text = r#"
            const config = {
              init() {},
              destroy() {},
              api: {
                baseUrl: "x",
                fetchData() {},
                async submitForm() {},
                errorHandler() {},
              },
              features: {
                auth: {
                  login() {},
                  logout() {},
                  refreshToken() {},
                },
                cache: {
                  get() {},
                  set() {},
                  invalidate() {},
                },
              },
              watch: {
                value() {},
              },
              computed: {
                fullName() {},
                displayValue() {},
              },
            };

            registerPlugin({
              name: "my-plugin",
              setup() {},
              teardown() {},
              hooks: {
                beforeMount() {},
                mounted() {},
                beforeUnmount() {},
              },
            });

            export const store = {
              state: {},
              mutations: {
                setUser() {},
                clearUser() {},
              },
              actions: {
                async fetchUser() {},
                logout() {},
              },
              getters: {
                currentUser() {},
                isAuthenticated() {},
              },
            };

            function registerPlugin(_plugin: unknown) {}
        "#
            .unindent();

            let buffer = cx.new(|cx| language::Buffer::local(text, cx).with_language(language, cx));
            cx.run_until_parked();
            let outline = buffer.read_with(cx, |buffer, _| buffer.snapshot().outline(None));

            let items: Vec<_> = outline
                .items
                .iter()
                .map(|item| (item.text.as_str(), item.depth))
                .collect();

            assert_eq!(
                items,
                &[
                    ("const config", 0),
                    ("init()", 1),
                    ("destroy()", 1),
                    ("api", 1),
                    ("baseUrl", 2),
                    ("fetchData()", 2),
                    ("async submitForm()", 2),
                    ("errorHandler()", 2),
                    ("features", 1),
                    ("auth", 2),
                    ("login()", 3),
                    ("logout()", 3),
                    ("refreshToken()", 3),
                    ("cache", 2),
                    ("get()", 3),
                    ("set()", 3),
                    ("invalidate()", 3),
                    ("watch", 1),
                    ("value()", 2),
                    ("computed", 1),
                    ("fullName()", 2),
                    ("displayValue()", 2),
                    ("name", 0),
                    ("setup()", 0),
                    ("teardown()", 0),
                    ("hooks", 0),
                    ("beforeMount()", 1),
                    ("mounted()", 1),
                    ("beforeUnmount()", 1),
                    ("const store", 0),
                    ("state", 1),
                    ("mutations", 1),
                    ("setUser()", 2),
                    ("clearUser()", 2),
                    ("actions", 1),
                    ("async fetchUser()", 2),
                    ("logout()", 2),
                    ("getters", 1),
                    ("currentUser()", 2),
                    ("isAuthenticated()", 2),
                    ("function registerPlugin( )", 0),
                ]
            );
        }
    }

    #[gpui::test]
    async fn test_outline_with_computed_property_names(cx: &mut TestAppContext) {
        for language in [
            crate::language(
                "typescript",
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            ),
            crate::language("tsx", tree_sitter_typescript::LANGUAGE_TSX.into()),
        ] {
            let text = r#"
            // Symbols as object keys
            const sym = Symbol("test");
            const obj1 = {
                [sym]: 1,
                [Symbol("inline")]: 2,
                normalKey: 3
            };

            // Enums as object keys
            enum Color { Red, Blue, Green }

            const obj2 = {
                [Color.Red]: "red value",
                [Color.Blue]: "blue value",
                regularProp: "normal"
            };

            // Mixed computed properties
            const key = "dynamic";
            const obj3 = {
                [key]: 1,
                ["string" + "concat"]: 2,
                [1 + 1]: 3,
                static: 4
            };

            // Nested objects with computed properties
            const obj4 = {
                [sym]: {
                    nested: 1
                },
                regular: {
                    [key]: 2
                }
            };
        "#
            .unindent();

            let buffer = cx.new(|cx| language::Buffer::local(text, cx).with_language(language, cx));
            let outline = buffer.read_with(cx, |buffer, _| buffer.snapshot().outline(None));
            assert_eq!(
                outline
                    .items
                    .iter()
                    .map(|item| (item.text.as_str(), item.depth))
                    .collect::<Vec<_>>(),
                &[
                    ("const sym", 0),
                    ("const obj1", 0),
                    ("[sym]", 1),
                    ("[Symbol(\"inline\")]", 1),
                    ("normalKey", 1),
                    ("enum Color", 0),
                    ("const obj2", 0),
                    ("[Color.Red]", 1),
                    ("[Color.Blue]", 1),
                    ("regularProp", 1),
                    ("const key", 0),
                    ("const obj3", 0),
                    ("[key]", 1),
                    ("[\"string\" + \"concat\"]", 1),
                    ("[1 + 1]", 1),
                    ("static", 1),
                    ("const obj4", 0),
                    ("[sym]", 1),
                    ("nested", 2),
                    ("regular", 1),
                    ("[key]", 2),
                ]
            );
        }
    }

    #[gpui::test]
    async fn test_generator_function_outline(cx: &mut TestAppContext) {
        let language = crate::language("javascript", tree_sitter_typescript::LANGUAGE_TSX.into());

        let text = r#"
            function normalFunction() {
                console.log("normal");
            }

            function* simpleGenerator() {
                yield 1;
                yield 2;
            }

            async function* asyncGenerator() {
                yield await Promise.resolve(1);
            }

            function* generatorWithParams(start, end) {
                for (let i = start; i <= end; i++) {
                    yield i;
                }
            }

            class TestClass {
                *methodGenerator() {
                    yield "method";
                }

                async *asyncMethodGenerator() {
                    yield "async method";
                }
            }
        "#
        .unindent();

        let buffer = cx.new(|cx| language::Buffer::local(text, cx).with_language(language, cx));
        let outline = buffer.read_with(cx, |buffer, _| buffer.snapshot().outline(None));
        assert_eq!(
            outline
                .items
                .iter()
                .map(|item| (item.text.as_str(), item.depth))
                .collect::<Vec<_>>(),
            &[
                ("function normalFunction()", 0),
                ("function* simpleGenerator()", 0),
                ("async function* asyncGenerator()", 0),
                ("function* generatorWithParams( )", 0),
                ("class TestClass", 0),
                ("*methodGenerator()", 1),
                ("async *asyncMethodGenerator()", 1),
            ]
        );
    }

    #[gpui::test]
    async fn test_conditional_test_wrappers(cx: &mut TestAppContext) {
        for language in [
            crate::language(
                "typescript",
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            ),
            crate::language("tsx", tree_sitter_typescript::LANGUAGE_TSX.into()),
            crate::language("javascript", tree_sitter_typescript::LANGUAGE_TSX.into()),
        ] {
            let text = r#"
                it.runIf(true)("runIf test", () => {
                    true;
                });

                it.skipIf(false)("skipIf test", () => {
                    true;
                });

                test.runIf(true)("runIf test 2", () => {
                    true;
                });

                test.skipIf(false)("skipIf test 2", () => {
                    true;
                });

                describe.runIf(true)("runIf describe", () => {
                    it("inner test", () => {
                        true;
                    });
                });

                describe.skipIf(false)("skipIf describe", () => {
                    it("inner test 2", () => {
                        true;
                    });
                });

                it.todoIf(false)("todoIf test", () => {
                    true;
                });

                it.if(true)("if test", () => {
                    true;
                });

                test.todoIf(false)("todoIf test 2", () => {
                    true;
                });

                test.if(true)("if test 2", () => {
                    true;
                });

                describe.todoIf(false)("todoIf describe", () => {
                    it("inner todoIf", () => {
                        true;
                    });
                });

                describe.if(true)("if describe", () => {
                    it("inner if", () => {
                        true;
                    });
                });

                test.failing("failing test", () => {
                    true;
                });

                it.failing("failing it", () => {
                    true;
                });

                it.each([1, 2, 3])("each test", () => {
                    true;
                });

                describe.each([1, 2])("each describe", () => {
                    it("inner each", () => {
                        true;
                    });
                });

                it.skip("skip test", () => {
                    true;
                });

                it.only("only test", () => {
                    true;
                });

                it.todo("todo test");
            "#
            .unindent();

            let text_len = text.len();
            let buffer = cx.new(|cx| language::Buffer::local(text, cx).with_language(language, cx));
            cx.executor().run_until_parked();

            let outline = buffer.update(cx, |buffer, _cx| buffer.snapshot().outline(None));
            let outline_names = outline
                .items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>();
            assert_eq!(
                outline_names,
                [
                    "runIf test",
                    "skipIf test",
                    "runIf test 2",
                    "skipIf test 2",
                    "runIf describe",
                    "it inner test",
                    "skipIf describe",
                    "it inner test 2",
                    "todoIf test",
                    "if test",
                    "todoIf test 2",
                    "if test 2",
                    "todoIf describe",
                    "it inner todoIf",
                    "if describe",
                    "it inner if",
                    "test.failing failing test",
                    "it.failing failing it",
                    "each test",
                    "each describe",
                    "it inner each",
                    "it.skip skip test",
                    "it.only only test",
                    "it.todo todo test",
                ]
            );

            let snapshot = buffer.update(cx, |buffer, _| buffer.snapshot());
            let runnable_names = snapshot
                .runnable_ranges(0..text_len)
                .map(|runnable| {
                    snapshot
                        .text_for_range(runnable.run_range)
                        .collect::<String>()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                runnable_names,
                [
                    "runIf test",
                    "skipIf test",
                    "runIf test 2",
                    "skipIf test 2",
                    "runIf describe",
                    "inner test",
                    "skipIf describe",
                    "inner test 2",
                    "todoIf test",
                    "if test",
                    "todoIf test 2",
                    "if test 2",
                    "todoIf describe",
                    "inner todoIf",
                    "if describe",
                    "inner if",
                    "failing test",
                    "failing it",
                    "each test",
                    "each describe",
                    "inner each",
                    "skip test",
                    "only test",
                    "todo test",
                ]
            );
        }
    }
}
