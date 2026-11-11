//! A read-only table over a vector of plain rows, each column a function - the model behind every
//! manager table (styles and their use counts, library definitions, survey-map rules), so none of
//! them keeps a second copy of its data in step by hand. The port of customisation/
//! row_table_model.hpp.
//!
//! Read-only on purpose: a manager's table shows, and a form beside it edits (docs/cad.md). Its
//! cells are enabled and selectable, never editable, and nothing here writes a row back.
//!
//! Reloading: a dialog rebuilds its rows (from DocumentWatcher's delivery, never inside a
//! Document notification) and calls `set_rows`, which is one reset; `index_of` then finds the row
//! it had selected by key, since a row index means nothing across a reload.
//!
//! Sorting: by each column's `Sort` value (QSortFilterProxyModel sorting by kSortRole). A column
//! that gives nothing for `Sort` sorts by its `Display` value, so a count column should answer
//! `Sort` with a number - "10" sorts before "9" as text.

/// The roles a column answers (Qt::ItemDataRole, and kSortRole).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Display,
    ToolTip,
    Foreground,
    Decoration,
    TextAlignment,
    /// The value a column sorts by (kSortRole).
    Sort,
}

/// What a cell answers for a role: QVariant's kinds the managers use.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Value {
    /// An invalid QVariant: the column does not answer the role.
    #[default]
    None,
    Text(String),
    Number(f64),
    Colour(crate::canvas::Color),
    /// An icon, by its name in the window's icon set.
    Icon(String),
}

impl Value {
    pub fn is_none(&self) -> bool {
        *self == Self::None
    }

    /// The value as text (QVariant::toString): a number as Qt writes a double, shortest.
    pub fn to_text(&self) -> String {
        match self {
            Self::None => String::new(),
            Self::Text(text) => text.clone(),
            Self::Number(number) => katana_core::text::format_exact_real(*number),
            Self::Colour(colour) => colour.name(),
            Self::Icon(name) => name.clone(),
        }
    }
}

/// A column's value for a row in a role (Qt's data(index, role) for that column).
pub type ColumnData<Row> = Box<dyn Fn(&Row, Role) -> Value>;

/// One column: its title, and its value for a row in a role.
pub struct Column<Row> {
    pub title: String,
    pub data: ColumnData<Row>,
}

impl<Row> Column<Row> {
    pub fn new(title: &str, data: impl Fn(&Row, Role) -> Value + 'static) -> Self {
        Self {
            title: String::from(title),
            data: Box::new(data),
        }
    }
}

/// A cell's flags (Qt::ItemFlags): enabled and selectable, never editable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ItemFlags {
    pub enabled: bool,
    pub selectable: bool,
    pub editable: bool,
}

/// The table (see the module comment).
pub struct RowTableModel<Row> {
    columns: Vec<Column<Row>>,
    rows: Vec<Row>,
    resets: u64,
}

impl<Row: 'static> RowTableModel<Row> {
    /// A column that shows `text(row)` and answers no other role.
    pub fn text_column(title: &str, text: impl Fn(&Row) -> String + 'static) -> Column<Row> {
        Column::new(title, move |row, role| {
            if role == Role::Display {
                Value::Text(text(row))
            } else {
                Value::None
            }
        })
    }

    pub fn new(columns: Vec<Column<Row>>) -> Self {
        Self {
            columns,
            rows: Vec::new(),
            resets: 0,
        }
    }

    /// Replaces every row: one reset, so a view drops its selection and the dialog restores it
    /// through `index_of`.
    pub fn set_rows(&mut self, rows: Vec<Row>) {
        self.rows = rows;
        self.resets += 1;
    }

    /// How many resets there have been (QAbstractItemModel::modelReset, counted).
    pub fn resets(&self) -> u64 {
        self.resets
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The row at a SOURCE row, or None.
    pub fn row_at(&self, row: usize) -> Option<&Row> {
        self.rows.get(row)
    }

    /// The first row `matches` accepts, or None.
    pub fn index_of(&self, matches: impl Fn(&Row) -> bool) -> Option<usize> {
        self.rows.iter().position(matches)
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// The value of a cell for `role`; none for a cell that is not there. `Sort` falls back to the
    /// cell's `Display` value when the column gives none.
    pub fn data(&self, row: usize, column: usize, role: Role) -> Value {
        let (Some(row), Some(column)) = (self.rows.get(row), self.columns.get(column)) else {
            return Value::None;
        };
        let value = (column.data)(row, role);
        if role == Role::Sort && value.is_none() {
            return (column.data)(row, Role::Display);
        }
        value
    }

    /// A column's title (headerData, horizontal, DisplayRole); None for a column that is not
    /// there.
    pub fn header_data(&self, section: usize) -> Option<&str> {
        self.columns
            .get(section)
            .map(|column| column.title.as_str())
    }

    /// Enabled and selectable for a cell that is there, nothing for one that is not.
    pub fn flags(&self, row: usize, column: usize) -> ItemFlags {
        if row < self.rows.len() && column < self.columns.len() {
            ItemFlags {
                enabled: true,
                selectable: true,
                editable: false,
            }
        } else {
            ItemFlags::default()
        }
    }

    /// QAbstractItemModel::setData: a read-only table takes nothing.
    pub fn set_data(&mut self, row: usize, column: usize, value: Value, role: Role) -> bool {
        let _ = (row, column, value, role);
        false
    }

    /// The source rows in the order sorting by `column` gives (QSortFilterProxyModel::sort by the
    /// Sort role): two numbers compare as numbers, anything else as its text, case-sensitively; a
    /// stable sort, so rows that compare equal keep their order; descending is the order reversed
    /// by the comparison, ties still in their order.
    pub fn sorted(&self, column: usize, descending: bool) -> Vec<usize> {
        let keys: Vec<Value> = (0..self.rows.len())
            .map(|row| self.data(row, column, Role::Sort))
            .collect();
        let mut order: Vec<usize> = (0..self.rows.len()).collect();
        order.sort_by(|&a, &b| {
            let ordering = compare_values(&keys[a], &keys[b]);
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
        order
    }

    /// Each row's cells as text (their Display values), for a named control's rows
    /// (drive::controls::rows) and a report.
    pub fn display_rows(&self) -> Vec<Vec<String>> {
        (0..self.rows.len())
            .map(|row| {
                (0..self.columns.len())
                    .map(|column| self.data(row, column, Role::Display).to_text())
                    .collect()
            })
            .collect()
    }
}

/// QSortFilterProxyModel's lessThan over two values: numbers as numbers, otherwise as text.
fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
        }
        _ => a.to_text().cmp(&b.to_text()),
    }
}
