mod core_xml {
    use katana_core::ErrorCode;
    use katana_core::xml::{XML_MAX_DEPTH, read_xml};

    #[test]
    fn trailing_content_and_non_scalar_references_follow_the_oracle() {
        assert_eq!(read_xml("<root/><!-- unfinished").unwrap().name, "root");
        let node = read_xml("<p>&#0;&#xD800;&#xDFFF;</p>").unwrap();
        assert_eq!(node.text, b"\0\xed\xa0\x80\xed\xbf\xbf");
    }

    fn nested(elements: usize) -> String {
        let mut text = String::new();
        for index in 0..elements {
            text.push_str(&format!("<a{index}>"));
        }
        for index in (0..elements).rev() {
            text.push_str(&format!("</a{index}>"));
        }
        text
    }

    #[test]
    fn reads_children_in_document_order_and_trims_their_text() {
        let root =
            read_xml("<root>\n  <first> one </first>\n  <second>two</second>\n</root>").unwrap();
        assert_eq!(root.name, "root");
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].name, "first");
        assert_eq!(root.children[1].name, "second");
        assert_eq!(root.child_text("first"), b"one");
        assert_eq!(root.child_text("second"), b"two");
        assert_eq!(root.text, b"");
    }

    #[test]
    fn nests_to_the_depth_of_the_document() {
        let root = read_xml("<a><b><c><d>deep</d></c></b></a>").unwrap();
        let child_b = root.child("b").unwrap();
        let child_c = child_b.child("c").unwrap();
        assert_eq!(child_c.child_text("d"), b"deep");
        assert!(child_c.has("d"));
        assert!(!child_c.has("b"));
    }

    #[test]
    fn finds_every_child_of_a_name() {
        let root =
            read_xml("<list><item>1</item><other/><item>2</item><item>3</item></list>").unwrap();
        let items = root.children_named("item");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].text, b"1");
        assert_eq!(items[1].text, b"2");
        assert_eq!(items[2].text, b"3");
        assert!(root.children_named("missing").is_empty());
    }

    #[test]
    fn an_empty_element_and_a_missing_one_are_the_same_answer() {
        let root = read_xml("<item><rotation/></item>").unwrap();
        assert_eq!(root.child_text("rotation"), b"");
        assert_eq!(root.child_text("scale"), b"");
        assert!(root.has("rotation"));
        assert!(!root.has("scale"));
    }

    #[test]
    fn reads_attributes_in_either_quote_in_the_order_written() {
        let root = read_xml("<p one=\"1\" two='2' three = \"3\" />").unwrap();
        assert_eq!(root.attributes.len(), 3);
        assert_eq!(root.attributes[0].0, "one");
        assert_eq!(root.attributes[0].1, b"1");
        assert_eq!(root.attributes[1].0, "two");
        assert_eq!(root.attributes[1].1, b"2");
        assert_eq!(root.attributes[2].0, "three");
        assert_eq!(root.attributes[2].1, b"3");
        let spaced = read_xml("<p name=\" kerb \"/>").unwrap();
        assert_eq!(spaced.attributes.len(), 1);
        assert_eq!(spaced.attributes[0].1, b" kerb ");
    }

    #[test]
    fn a_self_closing_element_is_an_element_with_no_text_and_no_children() {
        let root = read_xml("<set><line n='1'/><line n='2'/></set>").unwrap();
        assert_eq!(root.children.len(), 2);
        for line in &root.children {
            assert_eq!(line.name, "line");
            assert!(line.text.is_empty());
            assert!(line.children.is_empty());
            assert_eq!(line.attributes.len(), 1);
        }
        assert_eq!(root.children[0].attributes[0].1, b"1");
        assert_eq!(root.children[1].attributes[0].1, b"2");
    }

    #[test]
    fn a_cdata_section_is_taken_literally() {
        let root = read_xml("<note><![CDATA[ a < b & c ]]></note>").unwrap();
        assert_eq!(root.text, b"a < b & c");
    }

    #[test]
    fn cdata_and_plain_text_join_into_one_run() {
        let root = read_xml("<note>before<![CDATA[<middle>]]>after</note>").unwrap();
        assert_eq!(root.text, b"before<middle>after");
    }

    #[test]
    fn resolves_the_five_predefined_entities_in_text_and_in_attributes() {
        let root =
            read_xml("<t a=\"&amp;&lt;&gt;&quot;&apos;\">&amp;&lt;&gt;&quot;&apos;</t>").unwrap();
        assert_eq!(root.text, b"&<>\"'");
        assert_eq!(root.attributes.len(), 1);
        assert_eq!(root.attributes[0].1, b"&<>\"'");
    }

    #[test]
    fn resolves_decimal_and_hexadecimal_character_references() {
        let root = read_xml("<t>&#65;&#x42;&#233;&#x20AC;</t>").unwrap();
        assert_eq!(root.text, b"AB\xc3\xa9\xe2\x82\xac");
    }

    #[test]
    fn skips_the_declaration_comments_and_processing_instructions() {
        let root = read_xml(concat!(
            "<?xml version=\"1.0\"?>\n<!-- a note -->\n<?other pi?>\n",
            "<root><!-- inside --><a>1</a><?pi inside?></root>"
        ))
        .unwrap();
        assert_eq!(root.name, "root");
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.child_text("a"), b"1");
    }

    #[test]
    fn accepts_nesting_up_to_the_depth_cap_and_refuses_one_more() {
        assert!(read_xml(nested(XML_MAX_DEPTH + 1)).is_ok());
        let too_deep = read_xml(nested(XML_MAX_DEPTH + 2)).unwrap_err();
        assert_eq!(too_deep.code, ErrorCode::InvalidArgument);
        assert!(
            too_deep.message.contains("nested more than 64 deep"),
            "{}",
            too_deep.message
        );
    }

    #[test]
    fn refuses_a_doctype_rather_than_skipping_it() {
        let error = read_xml("<!DOCTYPE map SYSTEM \"map.dtd\">\n<map/>").unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.message.contains("document type declaration"),
            "{}",
            error.message
        );
        let subset = read_xml("<!DOCTYPE map [ <!ENTITY x \"y\"> ]>\n<map/>").unwrap_err();
        assert!(
            subset.message.contains("document type declaration"),
            "{}",
            subset.message
        );
    }

    #[test]
    fn refuses_an_entity_it_has_no_definition_for() {
        let error = read_xml("<t>&nbsp;</t>").unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("&nbsp;"), "{}", error.message);
        assert!(
            error.message.contains("entity this does not know"),
            "{}",
            error.message
        );
        let attribute = read_xml("<t a=\"&nbsp;\"/>").unwrap_err();
        assert!(
            attribute.message.contains("&nbsp;"),
            "{}",
            attribute.message
        );
    }

    #[test]
    fn refuses_an_ampersand_that_starts_no_entity() {
        let error = read_xml("<t>Jones & Son</t>").unwrap_err();
        assert!(error.message.contains("ampersand"), "{}", error.message);
    }

    #[test]
    fn refuses_a_character_reference_outside_unicode() {
        let error = read_xml("<t>&#x110000;</t>").unwrap_err();
        assert!(
            error.message.contains("is not a character"),
            "{}",
            error.message
        );
    }

    #[test]
    fn refuses_an_element_that_is_never_closed() {
        let text = read_xml("<root><a>one").unwrap_err();
        assert_eq!(text.code, ErrorCode::InvalidArgument);
        assert!(
            text.message.contains("\"a\" is never closed"),
            "{}",
            text.message
        );
        let tag = read_xml("<root><a ").unwrap_err();
        assert!(
            tag.message.contains("\"a\" is never closed"),
            "{}",
            tag.message
        );
    }

    #[test]
    fn refuses_a_mismatched_close_tag_and_names_the_line() {
        let error = read_xml("<a>\n  <b>\n  </a>\n").unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.message.contains("\"b\" is closed by \"a\""),
            "{}",
            error.message
        );
        assert_eq!(error.context, "line 3");
    }

    #[test]
    fn refuses_an_unclosed_comment_or_cdata_section() {
        let comment = read_xml("<!-- never ends\n<root/>").unwrap_err();
        assert!(
            comment.message.contains("comment is never closed"),
            "{}",
            comment.message
        );
        let cdata = read_xml("<root><![CDATA[ never ends </root>").unwrap_err();
        assert!(
            cdata.message.contains("CDATA section is never closed"),
            "{}",
            cdata.message
        );
    }

    #[test]
    fn refuses_a_malformed_attribute() {
        let bare = read_xml("<p flag/>").unwrap_err();
        assert!(bare.message.contains("has no value"), "{}", bare.message);
        let unquoted = read_xml("<p n=1/>").unwrap_err();
        assert!(
            unquoted.message.contains("is not in quotes"),
            "{}",
            unquoted.message
        );
        let unterminated = read_xml("<p n=\"1 />").unwrap_err();
        assert!(
            unterminated.message.contains("never closed"),
            "{}",
            unterminated.message
        );
    }

    #[test]
    fn refuses_text_that_is_not_an_element_at_all() {
        let empty = read_xml("").unwrap_err();
        assert!(
            empty.message.contains("no element here"),
            "{}",
            empty.message
        );
        let prose = read_xml("this is not XML").unwrap_err();
        assert!(
            prose.message.contains("no element here"),
            "{}",
            prose.message
        );
        let nameless = read_xml("<1>").unwrap_err();
        assert!(
            nameless.message.contains("tag has no name"),
            "{}",
            nameless.message
        );
    }
}
