//! The twins of tests/qt_widgets/customisation/test_shared_row_table_model.cpp: the read-only
//! table model over plain rows that the manager tables share.

use katana_window::customisation::{Column, Role, RowTableModel, Value};

mod row_table_model {
    use super::*;

    struct NameCount {
        name: String,
        count: i32,
    }

    fn row(name: &str, count: i32) -> NameCount {
        NameCount {
            name: String::from(name),
            count,
        }
    }

    type Model = RowTableModel<NameCount>;

    fn make_model() -> Model {
        let columns = vec![
            Model::text_column("Name", |row: &NameCount| row.name.clone()),
            Column::new("Used by", |row: &NameCount, role| match role {
                Role::Display => Value::Text(format!("{}", row.count)),
                Role::Sort => Value::Number(f64::from(row.count)),
                _ => Value::None,
            }),
        ];
        Model::new(columns)
    }

    #[test]
    fn each_column_shows_its_lambda_and_the_table_cannot_be_edited() {
        let mut model = make_model();
        model.set_rows(vec![row("Kerb", 9), row("Fence", 10)]);
        assert_eq!(model.row_count(), 2);
        assert_eq!(model.column_count(), 2);
        assert_eq!(model.header_data(1), Some("Used by"));
        assert_eq!(
            model.data(1, 0, Role::Display),
            Value::Text(String::from("Fence"))
        );
        assert_eq!(
            model.data(0, 1, Role::Display),
            Value::Text(String::from("9"))
        );
        assert!(!model.flags(0, 0).editable);
        assert!(!model.set_data(0, 0, Value::Text(String::from("x")), Role::Display));
        assert_eq!(model.row_at(1).map(|row| row.name.as_str()), Some("Fence"));
        assert!(model.row_at(2).is_none());
    }

    #[test]
    fn a_count_column_sorts_as_numbers_and_a_text_column_falls_back_to_its_text() {
        let mut model = make_model();
        model.set_rows(vec![row("b", 10), row("a", 9), row("c", 100)]);
        let name_of = |model: &Model, source: usize| model.data(source, 0, Role::Display);

        // As text "10" < "100" < "9"; as numbers 9 < 10 < 100.
        let ascending = model.sorted(1, false);
        assert_eq!(
            name_of(&model, ascending[0]),
            Value::Text(String::from("a"))
        );
        assert_eq!(
            name_of(&model, ascending[1]),
            Value::Text(String::from("b"))
        );
        assert_eq!(
            name_of(&model, ascending[2]),
            Value::Text(String::from("c"))
        );

        let descending = model.sorted(0, true);
        assert_eq!(
            name_of(&model, descending[0]),
            Value::Text(String::from("c"))
        );
    }

    #[test]
    fn reloading_resets_the_rows_and_index_of_finds_a_row_by_its_key() {
        let mut model = make_model();
        model.set_rows(vec![row("Kerb", 1)]);
        let before = model.resets();
        model.set_rows(vec![row("Fence", 2), row("Kerb", 3)]);
        assert_eq!(model.resets() - before, 1);
        assert_eq!(model.index_of(|row| row.name == "Kerb"), Some(1));
        assert_eq!(model.index_of(|row| row.name == "Wall"), None);
    }
}
