#[cfg(test)]
mod tests {
    mod core_xml {
        #[test]
        fn reads_nesting() {
            let _plain = "<a>}}</a> \" }";
            let _raw = r#"}" }"#;
            let _bytes = b"}";
            let _brace = '}';
            let _quote = '\'';
            // }
            /* } /* nested } */ } */
            fn _lifetime<'a>(text: &'a str) -> &'a str { text }
        }

        #[test]
        fn refuses_a_doctype() {}
    }
}
