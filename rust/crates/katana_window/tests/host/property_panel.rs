//! The Properties panel's tree (src/katana_window/src/property_panel.rs; property_panel.hpp): its
//! groups, a vertex's place with its attributes beneath it, a long level made a page at a time,
//! several entities read as one, the filter, and rows kept open across a refresh. Every expectation
//! is worked out by hand from the drawing each test types.

use std::thread;
use std::time::{Duration, Instant};

use katana_cad::{CommandInterpreter, Document};
use katana_window::drive::Harness;
use katana_window::host::{Clipboard, MemoryClipboard};
use katana_window::property_panel::{PROPERTY_PAGE, PropertyTreePanel, RowPath};

struct Panel {
    document: Document,
    interpreter: CommandInterpreter,
    panel: PropertyTreePanel,
}

impl Panel {
    fn new() -> Self {
        Self {
            document: Document::new(None),
            interpreter: CommandInterpreter::new(),
            panel: PropertyTreePanel::new(),
        }
    }

    fn run(&mut self, line: &str) {
        self.interpreter
            .run(&mut self.document, line)
            .unwrap_or_else(|error| panic!("{line}: {}", error.describe()));
    }

    fn show(&mut self) {
        self.panel.show_selection(&self.document);
    }

    /// The row whose first column says `label`, beneath `parent`; an empty path (no row) when
    /// there is none.
    fn row(&self, label: &str, parent: &[usize]) -> RowPath {
        self.panel.find_row(label, parent).unwrap_or_default()
    }

    /// A level's rows as "label | value", joined by " ; ".
    fn rows(&self, parent: &[usize]) -> String {
        (0..self.panel.row_count(parent))
            .map(|at| {
                let mut path = parent.to_vec();
                path.push(at);
                format!("{} | {}", self.panel.label(&path), self.panel.value(&path))
            })
            .collect::<Vec<_>>()
            .join(" ; ")
    }

    /// Opens a row as a click on its expander does.
    fn open(&mut self, path: &[usize]) {
        self.panel.expand(path, &self.document);
    }
}

/// A string of five vertices as an archive import leaves it: attributes on vertices 1 and 2, a
/// group, a code, and heights on all but the fourth.
fn type_the_string(p: &mut Panel) {
    p.run("PLINE 0,0 10,0 20,5 30,5 40,0");
    p.run("SELECT 1");
    p.run("PROP SET vertex/1/QL B");
    p.run("PROP SET vertex/1/Depth 0.6");
    p.run("PROP SET vertex/2/QL A");
    p.run("PROP SET Asset/Dimensions/Size 300");
    p.run("PROP SET Asset/Owner Council");
    p.run("PROP SET code KERB");
    p.run("PROP SET elevations \"1 2 3 null 5\"");
}

/// A string of 450 vertices along the x axis.
fn long_string() -> String {
    let mut line = String::from("PLINE");
    for i in 0..450 {
        line.push_str(&format!(" {i},0"));
    }
    line
}

/// The filter waits for a pause in the typing: frames of the panel until it has searched, two
/// seconds at most (the C++'s waitFor).
fn wait_for(
    harness: &mut Harness,
    panel: &mut PropertyTreePanel,
    document: &Document,
    clipboard: &mut MemoryClipboard,
    done: impl Fn(&PropertyTreePanel) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !done(panel) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
        harness
            .run(|ui| panel.ui(ui, document, &mut *clipboard))
            .expect("the panel is drawn");
    }
    done(panel)
}

mod property_panel {
    use super::*;

    #[test]
    fn nothing_selected_says_so_and_how_many_entities_there_are() {
        let mut p = Panel::new();
        p.run("LINE 0,0 1,1");
        p.run("LINE 0,1 1,2");
        p.run("SELECT NONE");
        p.show();
        assert_eq!(p.rows(&[]), "Selection | none ; Entities | 2");
    }

    #[test]
    fn one_string_is_its_groups_with_the_vertices_left_closed_until_opened() {
        let mut p = Panel::new();
        type_the_string(&mut p);
        p.show();
        // No Source group: the string was typed, not imported.
        assert_eq!(
            p.rows(&[]),
            "General |  ; Geometry |  ; Vertices | 5 vertices ; Attributes | 7 values"
        );
        for open in ["General", "Geometry", "Attributes"] {
            assert!(p.panel.is_expanded(&p.row(open, &[])), "{open}");
        }
        let vertices = p.row("Vertices", &[]);
        assert!(!p.panel.is_expanded(&vertices));
        // An expander, and nothing made beneath it until it is opened.
        assert!(p.panel.has_children(&vertices));
        assert_eq!(p.panel.row_count(&vertices), 0);

        assert_eq!(
            p.rows(&p.row("General", &[])),
            "Id | 1 ; Type | Polyline ; Layer | 0 ; Colour | ByLayer"
        );
        // The count is the Vertices group's; 10 + sqrt(125) + 10 + sqrt(125).
        assert_eq!(
            p.rows(&p.row("Geometry", &[])),
            "Closed | no ; Length | 42.3607"
        );
        // The vertices' attributes are under Vertices, and the heights too.
        assert_eq!(
            p.rows(&p.row("Attributes", &[])),
            "Asset | 2 values ; code | KERB ; elevations | see Vertices"
        );
    }

    #[test]
    fn a_vertex_is_its_place_and_height_with_its_attributes_beneath_it() {
        let mut p = Panel::new();
        type_the_string(&mut p);
        p.show();
        let vertices = p.row("Vertices", &[]);
        p.open(&vertices);
        // The fourth has no height ("null"): none shown, not a zero.
        assert_eq!(
            p.rows(&vertices),
            "Vertex 1 | 0.0000, 0.0000, 1.0000 ; Vertex 2 | 10.0000, 0.0000, 2.0000 ; \
             Vertex 3 | 20.0000, 5.0000, 3.0000 ; Vertex 4 | 30.0000, 5.0000 ; \
             Vertex 5 | 40.0000, 0.0000, 5.0000"
        );
        let first = p.row("Vertex 1", &vertices);
        assert!(p.panel.has_children(&first));
        assert!(!p.panel.has_children(&p.row("Vertex 3", &vertices)));
        p.open(&first);
        assert_eq!(p.rows(&first), "Depth | 0.6 ; QL | B");

        let asset = p.row("Asset", &p.row("Attributes", &[]));
        p.open(&asset);
        assert_eq!(p.rows(&asset), "Dimensions | 1 value ; Owner | Council");
    }

    #[test]
    fn a_long_string_is_made_a_page_at_a_time_with_a_show_more_row() {
        let mut p = Panel::new();
        // 450 vertices along the x axis, one attribute on the last.
        p.run(&long_string());
        p.run("SELECT 1");
        p.run("PROP SET vertex/450/QL D");
        p.show();
        let vertices = p.row("Vertices", &[]);
        assert_eq!(p.panel.value(&vertices), "450 vertices");
        p.open(&vertices);
        // One page and the Show more row: 450 = 200 + 200 + 50.
        assert_eq!(PROPERTY_PAGE, 200);
        assert_eq!(p.panel.row_count(&vertices), 201);
        let mut more = vertices.clone();
        more.push(200);
        assert_eq!(p.panel.label(&more), "Show 200 more");
        assert_eq!(p.panel.value(&more), "200 of 450 shown");

        // A click on it makes the next page where it is, once the click is over.
        p.panel.click(&more);
        assert_eq!(p.panel.row_count(&vertices), 201);
        p.panel.process_pending(&p.document);
        assert_eq!(p.panel.row_count(&vertices), 401);
        let mut again = vertices.clone();
        again.push(400);
        assert_eq!(p.panel.label(&again), "Show 50 more");
        assert_eq!(p.panel.value(&again), "400 of 450 shown");
        // Enter on it, as well as a click.
        p.panel.click(&again);
        p.panel.process_pending(&p.document);
        assert_eq!(p.panel.row_count(&vertices), 450);
        let mut last = vertices.clone();
        last.push(449);
        assert_eq!(p.panel.label(&last), "Vertex 450");
        assert_eq!(p.panel.value(&last), "449.0000, 0.0000");
        assert!(p.panel.has_children(&last));
        assert!(!p.panel.can_fetch_more(&vertices));
    }

    #[test]
    fn vertex_attributes_of_an_entity_with_no_vertex_list_are_in_number_order() {
        let mut p = Panel::new();
        p.run("POINT 0,0");
        p.run("SELECT 1");
        p.run("PROP SET vertex/10/QL C");
        p.run("PROP SET vertex/2/QL A");
        p.show();
        let attributes = p.row("Attributes", &[]);
        let vertex = p.row("vertex", &attributes);
        assert!(!vertex.is_empty(), "{}", p.rows(&attributes));
        p.open(&vertex);
        assert_eq!(p.rows(&vertex), "Vertex 2 | 1 value ; Vertex 10 | 1 value");
    }

    #[test]
    fn attributes_of_a_vertex_the_string_no_longer_has_stay_in_sight() {
        let mut p = Panel::new();
        p.run("PLINE 0,0 10,0 20,0");
        p.run("SELECT 1");
        p.run("PROP SET vertex/2/QL A");
        // Three vertices: there is no seventh.
        p.run("PROP SET vertex/7/QL Z");
        p.show();
        let attributes = p.row("Attributes", &[]);
        let vertex = p.row("vertex", &attributes);
        assert!(!vertex.is_empty(), "{}", p.rows(&attributes));
        p.open(&vertex);
        assert_eq!(p.rows(&vertex), "Vertex 2 | 1 value ; Vertex 7 | 1 value");
    }

    #[test]
    fn several_entities_show_what_they_share_and_where_they_vary() {
        let mut p = Panel::new();
        p.run("POINT 0,0");
        p.run("POINT 5,0");
        p.run("LINE 0,0 5,5");
        p.run("SELECT 1 2");
        p.run("PROP SET code TREE");
        p.run("SELECT 1");
        p.run("PROP SET height 1");
        p.run("SELECT 2");
        p.run("PROP SET height 2");
        p.run("SELECT 1 2 3");
        p.show();
        assert_eq!(
            p.rows(&p.row("General", &[])),
            "Selection | 3 entities ; Line | 1 ; Point | 2"
        );
        // The line holds neither, so both vary.
        assert_eq!(
            p.rows(&p.row("Attributes", &[])),
            "code | <varies> ; height | <varies>"
        );
        p.run("SELECT 1 2");
        p.show();
        assert_eq!(
            p.rows(&p.row("Attributes", &[])),
            "code | TREE ; height | <varies>"
        );
    }

    #[test]
    fn the_filter_lists_every_value_holding_the_words_however_deep() {
        let mut p = Panel::new();
        type_the_string(&mut p);
        p.show();
        let mut clipboard = MemoryClipboard::new();
        let mut harness = Harness::new();
        let Panel {
            document, panel, ..
        } = &mut p;
        harness
            .show(|ui| panel.ui(ui, document, &mut clipboard))
            .expect("the panel is drawn");
        harness
            .fill("propertyFilter", "ql")
            .run(|ui| panel.ui(ui, document, &mut clipboard))
            .expect("the filter is typed");
        // Seven values searched: the string's seven properties, and no source record, since it
        // was typed.
        assert!(wait_for(
            &mut harness,
            panel,
            document,
            &mut clipboard,
            |panel| panel.find_row("Matches", &[]).is_some()
        ));
        assert_eq!(
            panel.report(),
            "Matches | 2 of 7 values hold \"ql\" ; vertex/1/QL | B ; vertex/2/QL | A"
        );
        // A value matches as well as a name: "Council".
        harness
            .fill("propertyFilter", "COUN")
            .run(|ui| panel.ui(ui, document, &mut clipboard))
            .expect("the filter is typed");
        assert!(wait_for(
            &mut harness,
            panel,
            document,
            &mut clipboard,
            |panel| panel.report().contains("Asset/Owner")
        ));
        assert_eq!(
            panel.report(),
            "Matches | 1 of 7 values hold \"COUN\" ; Asset/Owner | Council"
        );
        // Cleared: the tree again, at once.
        harness
            .fill("propertyFilter", "")
            .run(|ui| panel.ui(ui, document, &mut clipboard))
            .expect("the filter is cleared");
        assert!(panel.find_row("Vertices", &[]).is_some());
        let attributes = panel.find_row("Attributes", &[]).expect("Attributes");
        assert!(panel.is_expanded(&attributes));
    }

    #[test]
    fn open_rows_stay_open_across_a_refresh_and_closed_groups_stay_closed() {
        let mut p = Panel::new();
        type_the_string(&mut p);
        p.show();
        let vertices = p.row("Vertices", &[]);
        p.open(&vertices);
        let first = p.row("Vertex 1", &vertices);
        p.open(&first);
        let general = p.row("General", &[]);
        p.panel.collapse(&general);
        // An edit elsewhere refreshes the panel, as the window does after every change; the same
        // string is shown.
        p.run("LAYER NEW other");
        p.show();
        let vertices = p.row("Vertices", &[]);
        assert!(p.panel.is_expanded(&vertices));
        assert!(p.panel.is_expanded(&p.row("Vertex 1", &vertices)));
        assert!(!p.panel.is_expanded(&p.row("General", &[])));

        // Another entity: its groups as they were left - General closed - and nothing deeper.
        p.run("POINT 1,1");
        p.run("SELECT 2");
        p.run("PROP SET code X");
        p.show();
        assert!(!p.panel.is_expanded(&p.row("General", &[])));
        assert!(p.panel.is_expanded(&p.row("Attributes", &[])));
    }

    #[test]
    fn copy_puts_the_selected_rows_on_the_clipboard_name_tab_value() {
        let mut p = Panel::new();
        type_the_string(&mut p);
        p.show();
        let code = p.row("code", &p.row("Attributes", &[]));
        assert!(!code.is_empty());
        p.panel.select(vec![code]);
        let mut clipboard = MemoryClipboard::new();
        // propertyCopy, triggered.
        p.panel.copy(&mut clipboard);
        assert_eq!(clipboard.text().as_deref(), Some("code\tKERB"));
    }

    #[test]
    fn opening_a_long_level_as_a_person_does_reads_its_first_page_only() {
        // Opened in the panel as it is drawn, as a person opens it: one page, and nothing read
        // twice when it is closed and opened again.
        let mut p = Panel::new();
        p.run(&long_string());
        p.run("SELECT 1");
        p.show();
        let mut clipboard = MemoryClipboard::new();
        let mut harness = Harness::with_screen(320.0, 600.0);
        let Panel {
            document, panel, ..
        } = &mut p;
        harness
            .show(|ui| panel.ui(ui, document, &mut clipboard))
            .expect("the panel is drawn");
        let vertices = panel.find_row("Vertices", &[]).expect("Vertices");
        panel.expand(&vertices, document);
        harness
            .run(|ui| panel.ui(ui, document, &mut clipboard))
            .expect("the panel is drawn");
        assert_eq!(panel.row_count(&vertices), 201);
        // Closed and opened again: nothing is read twice, no page is added.
        panel.collapse(&vertices);
        panel.expand(&vertices, document);
        harness
            .run(|ui| panel.ui(ui, document, &mut clipboard))
            .expect("the panel is drawn");
        assert_eq!(panel.row_count(&vertices), 201);
    }
}
