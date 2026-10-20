#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, TestAppContext};
    use unindent::Unindent;

    #[gpui::test]
    async fn test_outline(cx: &mut TestAppContext) {
        let language = crate::language("css", tree_sitter_css::LANGUAGE.into());

        let text = r#"
            /* Import statement */
            @import './fonts.css';

            /* multiline list of selectors with nesting */
            .test-class,
            div {
                .nested-class {
                    color: red;
                }
            }

            /* descendant selectors */
            .test .descendant {}

            /* pseudo */
            .test:not(:hover) {}

            /* media queries */
            @media screen and (min-width: 3000px) {
                .desktop-class {}
            }
        "#
        .unindent();

        let buffer = cx.new(|cx| {
            let mut buffer = language::Buffer::local(text, cx);
            buffer.set_sync_parse_timeout(None);
            buffer.set_language(Some(language), cx);
            buffer
        });
        buffer
            .read_with(cx, |buffer, _| buffer.parsing_idle())
            .await;
        let outline = buffer.read_with(cx, |buffer, _| buffer.snapshot().outline(None));
        assert_eq!(
            outline
                .items
                .iter()
                .map(|item| (item.text.as_str(), item.depth))
                .collect::<Vec<_>>(),
            &[
                ("@import './fonts.css'", 0),
                (".test-class, div", 0),
                (".nested-class", 1),
                (".test .descendant", 0),
                (".test:not(:hover)", 0),
                ("@media screen and (min-width: 3000px)", 0),
                (".desktop-class", 1),
            ]
        );
    }
}
