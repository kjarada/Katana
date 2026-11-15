//! What every part of the export shares: the file, the project frame (local
//! origin, placements, representation contexts), property sets, relationships
//! and the tallies of the report. Internal to katana_ifc.
//!
//! The parts (alignments in alignment.rs, services in utilities.rs, drawing
//! entities in drawing.rs, surfaces in surfaces.rs and the road model in
//! road.rs) each add their objects through it; the relationships that gather objects
//! from all of them (containment in the site, presentation layers,
//! classification) are written once, last, by finish().

use std::collections::{BTreeMap, BTreeSet};

use katana_entity::entity::{Color, PropertyMap, PropertyValue};
use katana_math::{Vec2, Vec3};

use crate::classification::IfcClass;
use crate::export::{ClassTally, ExportOptions, IfcExport};
use crate::product_layout::product_layout;
use crate::step::{character_count, clip_characters, guid_for, step_real, step_string};
use crate::step_file::{Args, Id, StepFile, list_of, typed_value};

/// IfcLabel and IfcIdentifier are STRING(255).
pub(crate) const LABEL_LIMIT: usize = 255;

/// The precision of the model context: the distance below which two points
/// are one, which the continuity checks of the buildingSMART validation
/// service also take as their tolerance (ALS016, ALS017). A tenth of a
/// millimetre is a hundred times finer than any survey Katana reduces and
/// far coarser than the arithmetic's error at the local coordinates written.
const CONTEXT_PRECISION: f64 = 1e-5;

fn label_or_text(value: &str) -> String {
    if character_count(value) > LABEL_LIMIT {
        typed_value("IfcText", &step_string(value))
    } else {
        typed_value("IfcLabel", &step_string(value))
    }
}

fn colour_key(colour: &Color) -> String {
    colour.to_hex()
}

/// One property of a property set, its value already encoded as a typed IFC
/// value (IFCLABEL('x')).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Property {
    pub(crate) name: String,
    /// encoded; empty for no value
    pub(crate) value: String,
    /// what the property means; empty for none
    pub(crate) specification: String,
    /// IfcPropertyEnumeratedValue instead of IfcPropertySingleValue: `value`
    /// is the one chosen, `enumeration` the IfcPropertyEnumeration listing the
    /// choices (0 for none), as the standard property sets and a delivery
    /// schema's domains have them.
    pub(crate) enumerated: bool,
    pub(crate) enumeration: Id,
}

impl Property {
    pub(crate) fn new(name: &str, value: String, specification: &str) -> Self {
        Self {
            name: String::from(name),
            value,
            specification: String::from(specification),
            enumerated: false,
            enumeration: 0,
        }
    }
}

/// A spatial element that holds products as the site holds the rest - an
/// IfcRoad or IfcRoadPart (road.rs) - with the placement its products hang
/// off: relative to the element's own. IFC 4.3.2 (8.7.3.14, IfcLocalPlacement)
/// places an element "to the local placement of its container, either
/// IfcSite, IfcFacility, or IfcFacilityPart", a facility relative to the
/// site, a facility part relative to its facility, and a feature relative to
/// the element it is tied to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Container {
    pub(crate) element: Id,
    /// the spatial element's own ObjectPlacement
    pub(crate) placement: Id,
    /// what its products are placed by, unmoved
    pub(crate) element_placement: Id,
    /// names its containment relationship's GlobalId
    pub(crate) key: String,
}

/// Properties in the order they are added. A value that is absent - an empty
/// text, None - adds nothing: a property with no value says less than no
/// property, and IFC's own property sets are read that way. (The C++'s
/// `ratio`, for an IfcRatioMeasure, had no caller and is not carried over.)
#[derive(Clone, Debug, Default)]
pub(crate) struct PropertyList {
    items: Vec<Property>,
}

impl PropertyList {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn add(&mut self, property: Property) -> &mut Self {
        self.items.push(property);
        self
    }

    /// IfcLabel, or IfcText when longer than an IfcLabel's 255 characters.
    pub(crate) fn label(&mut self, name: &str, value: &str) -> &mut Self {
        self.label_specified(name, value, "")
    }

    pub(crate) fn label_specified(
        &mut self,
        name: &str,
        value: &str,
        specification: &str,
    ) -> &mut Self {
        if value.is_empty() {
            return self;
        }
        self.add(Property::new(name, label_or_text(value), specification))
    }

    pub(crate) fn text(&mut self, name: &str, value: &str) -> &mut Self {
        if value.is_empty() {
            return self;
        }
        self.add(Property::new(
            name,
            typed_value("IfcText", &step_string(value)),
            "",
        ))
    }

    pub(crate) fn identifier(&mut self, name: &str, value: &str) -> &mut Self {
        if value.is_empty() {
            return self;
        }
        if character_count(value) > LABEL_LIMIT {
            return self.text(name, value);
        }
        self.add(Property::new(
            name,
            typed_value("IfcIdentifier", &step_string(value)),
            "",
        ))
    }

    pub(crate) fn length(&mut self, name: &str, value: Option<f64>) -> &mut Self {
        let Some(value) = value.filter(|value| value.is_finite()) else {
            return self;
        };
        self.add(Property::new(
            name,
            typed_value("IfcLengthMeasure", &step_real(value)),
            "",
        ))
    }

    /// IfcPositiveLengthMeasure: written only when the value is above zero.
    pub(crate) fn positive_length(&mut self, name: &str, value: Option<f64>) -> &mut Self {
        let Some(value) = value.filter(|value| value.is_finite() && *value > 0.0) else {
            return self;
        };
        self.add(Property::new(
            name,
            typed_value("IfcPositiveLengthMeasure", &step_real(value)),
            "",
        ))
    }

    pub(crate) fn positive_ratio(&mut self, name: &str, value: Option<f64>) -> &mut Self {
        let Some(value) = value.filter(|value| value.is_finite() && *value > 0.0) else {
            return self;
        };
        self.add(Property::new(
            name,
            typed_value("IfcPositiveRatioMeasure", &step_real(value)),
            "",
        ))
    }

    pub(crate) fn real(&mut self, name: &str, value: Option<f64>) -> &mut Self {
        let Some(value) = value.filter(|value| value.is_finite()) else {
            return self;
        };
        self.add(Property::new(
            name,
            typed_value("IfcReal", &step_real(value)),
            "",
        ))
    }

    pub(crate) fn integer(&mut self, name: &str, value: Option<i64>) -> &mut Self {
        let Some(value) = value else {
            return self;
        };
        self.add(Property::new(
            name,
            typed_value("IfcInteger", &format!("{value}")),
            "",
        ))
    }

    pub(crate) fn boolean(&mut self, name: &str, value: Option<bool>) -> &mut Self {
        let Some(value) = value else {
            return self;
        };
        self.add(Property::new(
            name,
            typed_value("IfcBoolean", if value { ".T." } else { ".F." }),
            "",
        ))
    }

    /// An IfcDate, "YYYY-MM-DD".
    pub(crate) fn date(&mut self, name: &str, iso_date: &str) -> &mut Self {
        if iso_date.is_empty() {
            return self;
        }
        self.add(Property::new(
            name,
            typed_value("IfcDate", &step_string(iso_date)),
            "",
        ))
    }

    pub(crate) fn enumerated(
        &mut self,
        name: &str,
        label: &str,
        enumeration: Id,
        specification: &str,
    ) -> &mut Self {
        if label.is_empty() {
            return self;
        }
        let mut property = Property::new(
            name,
            typed_value("IfcLabel", &step_string(label)),
            specification,
        );
        property.enumerated = true;
        property.enumeration = enumeration;
        self.add(property)
    }

    pub(crate) fn empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(crate) fn items(&self) -> &[Property] {
        &self.items
    }
}

/// An entity's own properties (the attribute manager's), each by its name and
/// typed as it is typed in Katana.
pub(crate) fn add_entity_properties(list: &mut PropertyList, properties: &PropertyMap) {
    for (name, value) in properties {
        if name.is_empty() || character_count(name) > LABEL_LIMIT {
            continue;
        }
        match value {
            PropertyValue::Text(text) => {
                list.label(name, text);
            }
            PropertyValue::Real(real) => {
                list.real(name, Some(*real));
            }
            PropertyValue::Integer(whole) => {
                list.integer(name, Some(*whole));
            }
            PropertyValue::Boolean(flag) => {
                list.boolean(name, Some(*flag));
            }
        }
    }
}

/// The frame the whole file is written in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Frame {
    /// the project coordinates of the file's (0, 0, 0)
    pub(crate) origin: Vec3,
    /// an IfcMapConversion records `origin`
    pub(crate) georeferenced: bool,
}

/// The six strings an account row is filed under: source, entity, predefined
/// type, object type, system, why.
type TallyKey = [String; 6];

pub(crate) struct Builder<'a> {
    options: &'a ExportOptions,
    frame: Frame,
    report: &'a mut IfcExport,
    file: StepFile,
    guids: BTreeSet<String>,

    origin2: Id,
    identity3: Id,
    unit_line: Id,
    project: Id,
    site: Id,
    site_placement: Id,
    element_placement: Id,
    axis_context: Id,
    body_context: Id,
    foot_print_context: Id,
    annotation_context: Id,
    metre: Id,
    kilogram: Id,

    contained: Vec<Id>,
    /// Products of the containers other than the site, by container, in the
    /// order the containers were written; and each container's key.
    contained_in: BTreeMap<Id, Vec<Id>>,
    container_keys: BTreeMap<Id, String>,
    referenced: Vec<Id>,
    aggregated_in_project: Vec<Id>,
    layers: BTreeMap<String, Vec<Id>>,
    classifications: BTreeMap<String, Id>,
    schedule_services: BTreeSet<String>,
    references: BTreeMap<(Id, String), Id>,
    /// a reference -> "<classification>/<identification>"
    reference_keys: BTreeMap<Id, String>,
    associations: BTreeMap<Id, Vec<Id>>,
    classification_keys: BTreeMap<Id, String>,
    tally: BTreeMap<TallyKey, usize>,
    enumerations: BTreeMap<String, Id>,
    curve_styles: BTreeMap<String, Id>,
    surface_styles: BTreeMap<String, Id>,
}

impl<'a> Builder<'a> {
    pub(crate) fn new(options: &'a ExportOptions, frame: Frame, report: &'a mut IfcExport) -> Self {
        let mut builder = Self {
            options,
            frame,
            report,
            file: StepFile::default(),
            guids: BTreeSet::new(),
            origin2: 0,
            identity3: 0,
            unit_line: 0,
            project: 0,
            site: 0,
            site_placement: 0,
            element_placement: 0,
            axis_context: 0,
            body_context: 0,
            foot_print_context: 0,
            annotation_context: 0,
            metre: 0,
            kilogram: 0,
            contained: Vec::new(),
            contained_in: BTreeMap::new(),
            container_keys: BTreeMap::new(),
            referenced: Vec::new(),
            aggregated_in_project: Vec::new(),
            layers: BTreeMap::new(),
            classifications: BTreeMap::new(),
            schedule_services: BTreeSet::new(),
            references: BTreeMap::new(),
            reference_keys: BTreeMap::new(),
            associations: BTreeMap::new(),
            classification_keys: BTreeMap::new(),
            tally: BTreeMap::new(),
            enumerations: BTreeMap::new(),
            curve_styles: BTreeMap::new(),
            surface_styles: BTreeMap::new(),
        };
        builder.write_project_frame();
        builder
    }

    pub(crate) fn file(&mut self) -> &mut StepFile {
        &mut self.file
    }

    /// The file as written: what write_ifc puts between DATA and ENDSEC.
    pub(crate) fn into_file(self) -> StepFile {
        self.file
    }

    pub(crate) fn options(&self) -> &'a ExportOptions {
        self.options
    }

    pub(crate) fn report(&mut self) -> &mut IfcExport {
        self.report
    }

    pub(crate) fn warn(&mut self, message: String) {
        self.report.warnings.push(message);
    }

    /// Accounts for one more object of `source` written as `ifc_class` (in
    /// `system`), for the reason `why` (IfcExport::tally); an empty class
    /// entity counts objects that were not written. finish() files the
    /// account in the report, in order. (The C++'s count parameter, always
    /// 1 at every caller, is not carried over.)
    pub(crate) fn tally(&mut self, source: &str, ifc_class: &IfcClass, system: &str, why: &str) {
        let key: TallyKey = [
            String::from(source),
            ifc_class.entity.clone(),
            ifc_class.predefined_type.clone(),
            ifc_class.object_type.clone(),
            String::from(system),
            String::from(why),
        ];
        *self.tally.entry(key).or_insert(0) += 1;
    }

    /// A GlobalId for `key`, unique in the file even if two keys hash alike
    /// (they never have; the check costs nothing).
    pub(crate) fn guid(&mut self, key: &str) -> String {
        let space = if self.options.guid_namespace.is_empty() {
            &self.options.project_name
        } else {
            &self.options.guid_namespace
        };
        let mut candidate = guid_for(space, key);
        let mut attempt = 1;
        while !self.guids.insert(candidate.clone()) {
            candidate = guid_for(space, &format!("{key}#{attempt}"));
            attempt += 1;
        }
        candidate
    }

    // ---- geometry, in the file's local coordinates --------------------------

    pub(crate) fn local(&self, world: Vec2) -> Vec2 {
        Vec2::new(world.x - self.frame.origin.x, world.y - self.frame.origin.y)
    }

    pub(crate) fn local_3d(&self, world: Vec3) -> Vec3 {
        Vec3::new(
            world.x - self.frame.origin.x,
            world.y - self.frame.origin.y,
            world.z - self.frame.origin.z,
        )
    }

    /// IfcCartesianPoint, 2D
    pub(crate) fn point(&mut self, world: Vec2) -> Id {
        let at = self.local(world);
        self.local_point(at)
    }

    /// already local
    pub(crate) fn local_point(&mut self, at: Vec2) -> Id {
        self.file
            .add("IfcCartesianPoint", Args::new().reals(&[at.x, at.y]))
    }

    /// IfcCartesianPoint, 3D
    pub(crate) fn point_3d(&mut self, world: Vec3) -> Id {
        let at = self.local_3d(world);
        self.file
            .add("IfcCartesianPoint", Args::new().reals(&[at.x, at.y, at.z]))
    }

    pub(crate) fn direction(&mut self, x: f64, y: f64) -> Id {
        self.file.add("IfcDirection", Args::new().reals(&[x, y]))
    }

    pub(crate) fn direction_3d(&mut self, x: f64, y: f64, z: f64) -> Id {
        self.file.add("IfcDirection", Args::new().reals(&[x, y, z]))
    }

    /// (0, 0), shared
    pub(crate) fn origin2(&mut self) -> Id {
        if self.origin2 == 0 {
            self.origin2 = self
                .file
                .add("IfcCartesianPoint", Args::new().reals(&[0.0, 0.0]));
        }
        self.origin2
    }

    /// IfcAxis2Placement2D
    pub(crate) fn placement2(&mut self, location: Id, angle: f64) -> Id {
        let direction = self.direction(angle.cos(), angle.sin());
        self.file.add(
            "IfcAxis2Placement2D",
            Args::new().reference(location).reference(direction),
        )
    }

    /// IfcAxis2Placement3D at (0, 0, 0), shared
    pub(crate) fn identity3(&mut self) -> Id {
        if self.identity3 == 0 {
            let at = self
                .file
                .add("IfcCartesianPoint", Args::new().reals(&[0.0, 0.0, 0.0]));
            self.identity3 = self.file.add(
                "IfcAxis2Placement3D",
                Args::new().reference(at).null().null(),
            );
        }
        self.identity3
    }

    /// IfcLine through (0, 0) along +x, shared: every straight
    /// IfcCurveSegment's parent
    pub(crate) fn unit_line(&mut self) -> Id {
        if self.unit_line == 0 {
            let direction = self.direction(1.0, 0.0);
            let vector = self
                .file
                .add("IfcVector", Args::new().reference(direction).real(1.0));
            let origin = self.origin2();
            self.unit_line = self
                .file
                .add("IfcLine", Args::new().reference(origin).reference(vector));
        }
        self.unit_line
    }

    pub(crate) fn polyline(&mut self, world: &[Vec2], closed: bool) -> Id {
        let mut points = Vec::with_capacity(world.len() + 1);
        for &p in world {
            points.push(self.point(p));
        }
        if closed && let Some(&first) = points.first() {
            points.push(first); // IfcPolyline closes by repeating its first point
        }
        self.file.add("IfcPolyline", Args::new().refs(&points))
    }

    pub(crate) fn polyline_3d(&mut self, world: &[Vec3], closed: bool) -> Id {
        let mut points = Vec::with_capacity(world.len() + 1);
        for &p in world {
            points.push(self.point_3d(p));
        }
        if closed && let Some(&first) = points.first() {
            points.push(first);
        }
        self.file.add("IfcPolyline", Args::new().refs(&points))
    }

    // ---- the project frame ----------------------------------------------------

    pub(crate) fn site(&self) -> Id {
        self.site
    }

    pub(crate) fn site_placement(&self) -> Id {
        self.site_placement
    }

    /// The placement every element and annotation hangs off: the site's
    /// frame, unmoved, shared.
    pub(crate) fn element_placement(&self) -> Id {
        self.element_placement
    }

    pub(crate) fn axis_context(&self) -> Id {
        self.axis_context
    }

    pub(crate) fn body_context(&self) -> Id {
        self.body_context
    }

    pub(crate) fn foot_print_context(&self) -> Id {
        self.foot_print_context
    }

    pub(crate) fn annotation_context(&self) -> Id {
        self.annotation_context
    }

    fn write_project_frame(&mut self) {
        let options = self.options;
        // Units: SI, lengths in metres and angles in radians - what every
        // length and angle in Katana already is, so nothing is scaled on the
        // way out. IfcSIUnit's Dimensions is derived from the unit type.
        let unit = |file: &mut StepFile, unit_type: &str, name: &str| {
            file.add(
                "IfcSIUnit",
                Args::new()
                    .derived()
                    .enumeration(unit_type)
                    .null()
                    .enumeration(name),
            )
        };
        self.metre = unit(&mut self.file, "LENGTHUNIT", "METRE");
        let square_metre = unit(&mut self.file, "AREAUNIT", "SQUARE_METRE");
        let cubic_metre = unit(&mut self.file, "VOLUMEUNIT", "CUBIC_METRE");
        let radian = unit(&mut self.file, "PLANEANGLEUNIT", "RADIAN");
        let units = self.file.add(
            "IfcUnitAssignment",
            Args::new().refs(&[self.metre, square_metre, cubic_metre, radian]),
        );

        // One 3D model context; true north is left to its default, grid
        // north (+y), which is what a projected coordinate system's y axis is.
        let identity = self.identity3();
        let context = self.file.add(
            "IfcGeometricRepresentationContext",
            Args::new()
                .null()
                .string("Model")
                .integer(3)
                .real(CONTEXT_PRECISION)
                .reference(identity)
                .null(),
        );
        // The subcontexts the representations name. A subcontext's dimension,
        // precision, coordinate system and true north are its parent's
        // (derived, *). Axis is a GRAPH_VIEW, as the IFC 4.3 alignment
        // examples have it.
        let sub_context = |file: &mut StepFile, identifier: &str, view: &str| {
            file.add(
                "IfcGeometricRepresentationSubContext",
                Args::new()
                    .string(identifier)
                    .string("Model")
                    .derived()
                    .derived()
                    .derived()
                    .derived()
                    .reference(context)
                    .null()
                    .enumeration(view)
                    .null(),
            )
        };
        self.axis_context = sub_context(&mut self.file, "Axis", "GRAPH_VIEW");
        self.body_context = sub_context(&mut self.file, "Body", "MODEL_VIEW");
        self.foot_print_context = sub_context(&mut self.file, "FootPrint", "MODEL_VIEW");
        self.annotation_context = sub_context(&mut self.file, "Annotation", "MODEL_VIEW");

        if self.frame.georeferenced {
            let crs = &options.georeference;
            let projected = self.file.add(
                "IfcProjectedCRS",
                Args::new()
                    .string(&crs.name)
                    .string_or_null(&crs.description)
                    .string_or_null(&crs.geodetic_datum)
                    .string_or_null(&crs.vertical_datum)
                    .string_or_null(&crs.map_projection)
                    .string_or_null(&crs.map_zone)
                    .reference(self.metre),
            );
            // The file's axes are the grid's (no rotation: abscissa 1,
            // ordinate 0) at the grid's scale: Katana's coordinates ARE grid
            // coordinates, so no ground-to-grid factor applies, and the length
            // units are the CRS's (metres), so the scale is 1 (GRF005).
            self.file.add(
                "IfcMapConversion",
                Args::new()
                    .reference(context)
                    .reference(projected)
                    .real(self.frame.origin.x)
                    .real(self.frame.origin.y)
                    .real(self.frame.origin.z)
                    .real(1.0)
                    .real(0.0)
                    .real(1.0),
            );
        }

        let project_guid = self.guid("project");
        self.project = self.file.add(
            "IfcProject",
            Args::new()
                .string(&project_guid)
                .null()
                .string(&options.project_name)
                .string_or_null(&options.project_description)
                .null()
                .null()
                .null()
                .refs(&[context])
                .reference(units),
        );
        let identity = self.identity3();
        self.site_placement = self
            .file
            .add("IfcLocalPlacement", Args::new().null().reference(identity));
        let site_guid = self.guid("site");
        self.site = self.file.add(
            "IfcSite",
            Args::new()
                .string(&site_guid)
                .null()
                .string(&options.site_name)
                .null()
                .null()
                .reference(self.site_placement)
                .null()
                .null()
                .enumeration("ELEMENT")
                .null()
                .null()
                .null()
                .null()
                .null(),
        );
        let identity = self.identity3();
        self.element_placement = self.file.add(
            "IfcLocalPlacement",
            Args::new()
                .reference(self.site_placement)
                .reference(identity),
        );
    }

    pub(crate) fn shape(&mut self, context: Id, identifier: &str, kind: &str, items: &[Id]) -> Id {
        self.file.add(
            "IfcShapeRepresentation",
            Args::new()
                .reference(context)
                .string(identifier)
                .string(kind)
                .refs(items),
        )
    }

    pub(crate) fn product_shape(&mut self, representations: &[Id]) -> Id {
        self.file.add(
            "IfcProductDefinitionShape",
            Args::new().null().null().refs(representations),
        )
    }

    /// `text` as an IfcLabel: at most 255 characters, a cut said in the
    /// warnings naming `what` - for the names written outside product().
    pub(crate) fn label(&mut self, text: &str, what: &str) -> String {
        if character_count(text) <= LABEL_LIMIT {
            return String::from(text);
        }
        self.warn(format!(
            "{what} is longer than the 255 characters IFC allows it and is cut there"
        ));
        clip_characters(text, LABEL_LIMIT)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "The attributes of the C++'s writeProduct, in its order."
    )]
    fn write_product(
        &mut self,
        ifc_class: &IfcClass,
        key: &str,
        name: &str,
        description: &str,
        representation: Id,
        tag: &str,
        placement: Id,
    ) -> Id {
        let layout = product_layout(&ifc_class.entity);
        let of = format!("{} {key}", ifc_class.entity);
        // In the C++'s order of evaluation: the GlobalId, then each label,
        // whose cut is a warning.
        let guid = self.guid(key);
        let name = self.label(name, &format!("{of}: its name"));
        let object_type = self.label(&ifc_class.object_type, &format!("{of}: its object type"));
        let mut args = Args::new();
        args.string(&guid)
            .null()
            .string_or_null(&name)
            .string_or_null(description)
            .string_or_null(&object_type)
            .reference(placement)
            .reference_or_null(representation);
        if layout.has_tag {
            let tag = self.label(tag, &format!("{of}: its tag"));
            args.string_or_null(&tag);
        }
        if layout.has_predefined_type {
            args.enumeration_or_null(&ifc_class.predefined_type);
        }
        let id = self.file.add(&ifc_class.entity, &args);
        *self
            .report
            .classes
            .entry(ifc_class.entity.clone())
            .or_insert(0) += 1;
        id
    }

    /// An element or annotation instance of `ifc_class`, placed at
    /// element_placement(), and contained in the site. `tag` is written where
    /// the class has a Tag. A name, object type or tag longer than IFC's 255
    /// characters (IfcLabel, IfcIdentifier) is cut there, and the cut said in
    /// the warnings.
    pub(crate) fn product(
        &mut self,
        ifc_class: &IfcClass,
        key: &str,
        name: &str,
        description: &str,
        representation: Id,
        tag: &str,
    ) -> Id {
        let placement = self.element_placement;
        let id = self.write_product(
            ifc_class,
            key,
            name,
            description,
            representation,
            tag,
            placement,
        );
        self.contained.push(id);
        id
    }

    /// The same, contained in `container` and placed by its
    /// element_placement.
    #[allow(
        clippy::too_many_arguments,
        reason = "The attributes of the C++'s product, in its order."
    )]
    pub(crate) fn product_in(
        &mut self,
        ifc_class: &IfcClass,
        key: &str,
        name: &str,
        description: &str,
        representation: Id,
        tag: &str,
        container: &Container,
    ) -> Id {
        if container.element == self.site {
            return self.product(ifc_class, key, name, description, representation, tag);
        }
        let id = self.write_product(
            ifc_class,
            key,
            name,
            description,
            representation,
            tag,
            container.element_placement,
        );
        self.contained_in
            .entry(container.element)
            .or_default()
            .push(id);
        self.container_keys
            .insert(container.element, container.key.clone());
        id
    }

    /// A feature element (ProductLayout::feature): placed by `placement` -
    /// relative to its host's - and contained nowhere; the caller writes its
    /// IfcRelVoidsElement or IfcRelAdheresToElement.
    #[allow(
        clippy::too_many_arguments,
        reason = "The attributes of the C++'s feature, in its order."
    )]
    pub(crate) fn feature(
        &mut self,
        ifc_class: &IfcClass,
        key: &str,
        name: &str,
        description: &str,
        representation: Id,
        tag: &str,
        placement: Id,
    ) -> Id {
        self.write_product(
            ifc_class,
            key,
            name,
            description,
            representation,
            tag,
            placement,
        )
    }

    /// An IfcLocalPlacement relative to `relative_to`, unmoved.
    pub(crate) fn placement_in(&mut self, relative_to: Id) -> Id {
        let identity = self.identity3();
        self.file.add(
            "IfcLocalPlacement",
            Args::new().reference(relative_to).reference(identity),
        )
    }

    /// The site as a Container, for what is placed relative to it.
    pub(crate) fn site_container(&self) -> Container {
        Container {
            element: self.site,
            placement: self.site_placement,
            element_placement: self.element_placement,
            key: String::from("site"),
        }
    }

    /// An IfcRelAggregates from `whole` to `parts`, written now (nothing when
    /// `parts` is empty). `key` names it, so its GlobalId survives a
    /// re-export.
    pub(crate) fn aggregate(&mut self, whole: Id, key: &str, parts: &[Id]) {
        if parts.is_empty() {
            return;
        }
        let guid = self.guid(&format!("aggregates/{key}"));
        self.file.add(
            "IfcRelAggregates",
            Args::new()
                .string(&guid)
                .null()
                .null()
                .null()
                .reference(whole)
                .refs(parts),
        );
    }

    /// The unit a weight is given in: IfcSIUnit KILO GRAM, written once. The
    /// project's units (write_project_frame) have no mass unit, so a weight
    /// names its own rather than leave a reader to guess.
    pub(crate) fn kilogram(&mut self) -> Id {
        if self.kilogram == 0 {
            self.kilogram = self.file.add(
                "IfcSIUnit",
                Args::new()
                    .derived()
                    .enumeration("MASSUNIT")
                    .enumeration("KILO")
                    .enumeration("GRAM"),
            );
        }
        self.kilogram
    }

    /// The services of the schedule written (UTILITIES), by "<line id>/<type>",
    /// so that the same service drawn in the drawing can be said to be twice.
    pub(crate) fn note_schedule_service(&mut self, service: String) {
        self.schedule_services.insert(service);
    }

    pub(crate) fn is_schedule_service(&self, service: &str) -> bool {
        self.schedule_services.contains(service)
    }

    // ---- properties -------------------------------------------------------------

    /// An IfcPropertySet, or 0 when the list is empty.
    pub(crate) fn property_set(&mut self, key: &str, name: &str, properties: &PropertyList) -> Id {
        if properties.empty() {
            return 0;
        }
        let mut ids = Vec::with_capacity(properties.items().len());
        for property in properties.items() {
            let mut args = Args::new();
            args.string(&property.name)
                .string_or_null(&property.specification);
            if property.enumerated {
                args.raw(&list_of(std::slice::from_ref(&property.value)))
                    .reference_or_null(property.enumeration);
                ids.push(self.file.add("IfcPropertyEnumeratedValue", &args));
            } else {
                if property.value.is_empty() {
                    args.null();
                } else {
                    args.raw(&property.value);
                }
                args.null();
                ids.push(self.file.add("IfcPropertySingleValue", &args));
            }
        }
        let guid = self.guid(&format!("pset/{key}"));
        self.file.add(
            "IfcPropertySet",
            Args::new()
                .string(&guid)
                .null()
                .string(name)
                .null()
                .refs(&ids),
        )
    }

    /// IfcRelDefinesByProperties from `set` to `objects` (nothing when either
    /// is empty). `key` names the relationship, as property_set's key names
    /// the set, so that its GlobalId survives a re-export.
    pub(crate) fn defines(&mut self, key: &str, set: Id, objects: &[Id]) {
        if set == 0 || objects.is_empty() {
            return;
        }
        let guid = self.guid(&format!("defines/{key}"));
        self.file.add(
            "IfcRelDefinesByProperties",
            Args::new()
                .string(&guid)
                .null()
                .null()
                .null()
                .refs(objects)
                .reference(set),
        );
    }

    /// The IfcPropertyEnumeration called `name`, written once.
    pub(crate) fn enumeration(&mut self, name: &str, encoded_values: &[String]) -> Id {
        if let Some(&found) = self.enumerations.get(name) {
            return found;
        }
        let id = self.file.add(
            "IfcPropertyEnumeration",
            Args::new()
                .string(name)
                .raw(&list_of(encoded_values))
                .null(),
        );
        self.enumerations.insert(String::from(name), id);
        id
    }

    // ---- relationships written by finish() ---------------------------------------

    pub(crate) fn reference_in_site(&mut self, product: Id) {
        self.referenced.push(product);
    }

    pub(crate) fn aggregate_in_project(&mut self, object: Id) {
        self.aggregated_in_project.push(object);
    }

    pub(crate) fn layer(&mut self, name: &str, representation: Id) {
        if !name.is_empty() && representation != 0 {
            self.layers
                .entry(String::from(name))
                .or_default()
                .push(representation);
        }
    }

    /// An IfcClassification, written once per key.
    pub(crate) fn classification(
        &mut self,
        key: &str,
        source: &str,
        edition: &str,
        name: &str,
        description: &str,
    ) -> Id {
        if let Some(&found) = self.classifications.get(key) {
            return found;
        }
        let id = self.file.add(
            "IfcClassification",
            Args::new()
                .string_or_null(source)
                .string_or_null(edition)
                .null()
                .string(name)
                .string_or_null(description)
                .null()
                .null(),
        );
        self.classifications.insert(String::from(key), id);
        self.classification_keys.insert(id, String::from(key));
        id
    }

    /// A reference into it, written once per (classification, identification).
    pub(crate) fn classification_reference(
        &mut self,
        classification: Id,
        identification: &str,
        name: &str,
    ) -> Id {
        let key = (classification, String::from(identification));
        if let Some(&found) = self.references.get(&key) {
            return found;
        }
        let id = self.file.add(
            "IfcClassificationReference",
            Args::new()
                .null()
                .string(identification)
                .string_or_null(name)
                .reference(classification)
                .null()
                .null(),
        );
        self.references.insert(key, id);
        let classification_key = self
            .classification_keys
            .get(&classification)
            .cloned()
            .unwrap_or_default();
        self.reference_keys
            .insert(id, format!("{classification_key}/{identification}"));
        id
    }

    pub(crate) fn associate(&mut self, reference: Id, object: Id) {
        if reference != 0 && object != 0 {
            self.associations.entry(reference).or_default().push(object);
        }
    }

    /// An IfcRelAssignsToGroup from `group` to `members`.
    pub(crate) fn group(&mut self, group: Id, key: &str, members: &[Id]) {
        if members.is_empty() {
            return;
        }
        let guid = self.guid(&format!("group/{key}"));
        self.file.add(
            "IfcRelAssignsToGroup",
            Args::new()
                .string(&guid)
                .null()
                .null()
                .null()
                .refs(members)
                .null()
                .reference(group),
        );
    }

    // ---- presentation ----------------------------------------------------------------

    fn rgb(&mut self, colour: &Color) -> Id {
        self.file.add(
            "IfcColourRgb",
            Args::new()
                .null()
                .real(f64::from(colour.r) / 255.0)
                .real(f64::from(colour.g) / 255.0)
                .real(f64::from(colour.b) / 255.0),
        )
    }

    /// Styles `item` with the colour: a curve style for a curve or text. One
    /// style per colour.
    pub(crate) fn colour_curve(&mut self, item: Id, colour: &Color) {
        let key = colour_key(colour);
        let existing = self.curve_styles.get(&key).copied();
        let style = match existing {
            Some(style) => style,
            None => {
                let rgb = self.rgb(colour);
                let style = self.file.add(
                    "IfcCurveStyle",
                    Args::new()
                        .string(&key)
                        .null()
                        .null()
                        .reference(rgb)
                        .boolean(true),
                );
                self.curve_styles.insert(key, style);
                style
            }
        };
        self.file.add(
            "IfcStyledItem",
            Args::new().reference(item).refs(&[style]).null(),
        );
    }

    /// A surface style for a body. One style per colour.
    pub(crate) fn colour_surface(&mut self, item: Id, colour: &Color) {
        let key = colour_key(colour);
        let existing = self.surface_styles.get(&key).copied();
        let style = match existing {
            Some(style) => style,
            None => {
                let rgb = self.rgb(colour);
                let shading = self
                    .file
                    .add("IfcSurfaceStyleShading", Args::new().reference(rgb).null());
                let style = self.file.add(
                    "IfcSurfaceStyle",
                    Args::new()
                        .string(&key)
                        .enumeration("BOTH")
                        .refs(&[shading]),
                );
                self.surface_styles.insert(key, style);
                style
            }
        };
        self.file.add(
            "IfcStyledItem",
            Args::new().reference(item).refs(&[style]).null(),
        );
    }

    pub(crate) fn finish(&mut self) {
        // The site, and every alignment: IFC 4.3 aggregates an alignment into
        // the project and does not contain it in a spatial element (ALB004,
        // SPS007).
        let mut parts = vec![self.site];
        parts.extend_from_slice(&self.aggregated_in_project);
        let guid = self.guid("aggregates/project");
        self.file.add(
            "IfcRelAggregates",
            Args::new()
                .string(&guid)
                .null()
                .null()
                .null()
                .reference(self.project)
                .refs(&parts),
        );
        if !self.contained.is_empty() {
            let guid = self.guid("contained/site");
            self.file.add(
                "IfcRelContainedInSpatialStructure",
                Args::new()
                    .string(&guid)
                    .null()
                    .null()
                    .null()
                    .refs(&self.contained)
                    .reference(self.site),
            );
        }
        // The road model's facilities and their parts, each holding its own
        // (road.rs): one relationship a container, in the order written.
        let contained_in = std::mem::take(&mut self.contained_in);
        for (container, products) in &contained_in {
            let key = self
                .container_keys
                .get(container)
                .cloned()
                .unwrap_or_default();
            let guid = self.guid(&format!("contained/{key}"));
            self.file.add(
                "IfcRelContainedInSpatialStructure",
                Args::new()
                    .string(&guid)
                    .null()
                    .null()
                    .null()
                    .refs(products)
                    .reference(*container),
            );
        }
        self.contained_in = contained_in;
        if !self.referenced.is_empty() {
            let guid = self.guid("referenced/site");
            self.file.add(
                "IfcRelReferencedInSpatialStructure",
                Args::new()
                    .string(&guid)
                    .null()
                    .null()
                    .null()
                    .refs(&self.referenced)
                    .reference(self.site),
            );
        }
        for (name, representations) in &self.layers {
            self.file.add(
                "IfcPresentationLayerAssignment",
                Args::new().string(name).null().refs(representations).null(),
            );
        }
        self.report.tally.clear();
        self.report.tally.reserve(self.tally.len());
        for (key, count) in &self.tally {
            self.report.tally.push(ClassTally {
                source: key[0].clone(),
                entity: key[1].clone(),
                predefined_type: key[2].clone(),
                object_type: key[3].clone(),
                system: key[4].clone(),
                why: key[5].clone(),
                count: *count,
            });
        }
        let associations = std::mem::take(&mut self.associations);
        for (reference, objects) in &associations {
            let key = self
                .reference_keys
                .get(reference)
                .cloned()
                .unwrap_or_default();
            let guid = self.guid(&format!("classified/{key}"));
            self.file.add(
                "IfcRelAssociatesClassification",
                Args::new()
                    .string(&guid)
                    .null()
                    .null()
                    .null()
                    .refs(objects)
                    .reference(*reference),
            );
        }
        self.associations = associations;
    }
}
