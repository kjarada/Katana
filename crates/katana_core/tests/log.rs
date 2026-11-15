mod core_log {
    use katana_core::log::{LogField, LogLevel, LogRecord, Logger, format_record};
    use std::sync::{Arc, Mutex};
    use std::time::UNIX_EPOCH;

    #[test]
    fn delivers_structured_records_to_sinks() {
        let logger = Logger::new();
        let records = Arc::new(Mutex::new(Vec::new()));
        let sink_records = Arc::clone(&records);
        logger.add_sink(move |record| sink_records.lock().unwrap().push(record.clone()));
        logger.info(
            "command",
            "executed",
            &[
                LogField::new("name", "CREATE_LINE"),
                LogField::new("entities", "1"),
            ],
        );
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].level, LogLevel::Info);
        assert_eq!(records[0].category, "command");
        assert_eq!(records[0].fields.len(), 2);
        assert_eq!(records[0].fields[0].key, "name");
        assert_eq!(records[0].fields[0].value, "CREATE_LINE");
    }

    #[test]
    fn filters_below_minimum_level() {
        let logger = Logger::new();
        let delivered = Arc::new(Mutex::new(0));
        let sink_delivered = Arc::clone(&delivered);
        logger.add_sink(move |_| *sink_delivered.lock().unwrap() += 1);
        logger.debug("storage", "ignored at default Info level", &[]);
        assert_eq!(*delivered.lock().unwrap(), 0);
        logger.set_minimum_level(LogLevel::Trace);
        logger.trace("storage", "now visible", &[]);
        logger.critical("storage", "always visible", &[]);
        assert_eq!(*delivered.lock().unwrap(), 2);
    }

    #[test]
    fn formats_record_with_quoted_field_values() {
        let record = LogRecord {
            time: UNIX_EPOCH,
            level: LogLevel::Warning,
            category: "import".into(),
            message: "skipped rows".into(),
            fields: vec![
                LogField::new("file", "site survey.csv"),
                LogField::new("rows", "3"),
            ],
        };
        assert_eq!(
            format_record(&record),
            "1970-01-01T00:00:00.000Z WARNING [import] skipped rows file=\"site survey.csv\" rows=3"
        );
    }

    #[test]
    fn field_escaping_keeps_the_oracles_quoting_rules() {
        let record = LogRecord {
            category: "test".into(),
            message: "fields".into(),
            fields: vec![
                LogField::new("empty", ""),
                LogField::new("quote", "a\"b"),
                LogField::new("slash", r"C:\new"),
                LogField::new("spaced", r"C:\new dir"),
                LogField::new("equals", "a=b"),
                LogField::new("tab", "a\tb"),
                LogField::new("newline", "a\nb"),
            ],
            ..LogRecord::default()
        };
        assert_eq!(
            format_record(&record),
            "1970-01-01T00:00:00.000Z INFO [test] fields empty=\"\" quote=\"a\\\"b\" slash=C:\\new spaced=\"C:\\\\new dir\" equals=\"a=b\" tab=\"a\tb\" newline=a\nb"
        );
    }
}
