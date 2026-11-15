//! Services of an AS 5488 subsurface utility investigation -> IFC.
//!
//! A SERVICE is an IfcDistributionSystem (WATERSUPPLY, SEWAGE, ELECTRICAL ...,
//! classification.rs) grouping:
//!
//! * one element per GRADED SEGMENT - an IfcPipeSegment, IfcCableSegment,
//!   IfcCableCarrierSegment or IfcDistributionFlowElement - because AS 5488
//!   grades a segment, not a service: a water main verified by a pothole is
//!   QL-A for a metre and QL-B either side, and a single element carrying one
//!   level would claim too much or too little along most of its length
//!   (docs/subsurface_utilities.md, "Results are per segment");
//! * one IfcAnnotation SURVEY per LOCATED POINT, where the evidence is: its
//!   method, the level it supports, what it claims, its assessed uncertainty
//!   and the depth of cover there;
//! * for a service recorded at one point, the element its feature names - a
//!   pit, a valve, a hydrant (classify_utility_point).
//!
//! What each carries:
//!
//! ```text
//! AS5488_QualityLevel   the segment's graded level, the level claimed for
//!                       it, whether the claim holds, what limits it, the
//!                       evidence along it, its plan length
//! AS5488_LocatedPoint   the point's method, attained and claimed levels,
//!                       the reasons it is below its method's ceiling,
//!                       levels, depth and cover
//! AS5488_Service        the service: type, owner, material, size,
//!                       configuration, status, and its length at each level
//! Pset_Uncertainty      IFC's own statement of positional uncertainty: for
//!                       a segment the AS 5488 tolerance of its level, for a
//!                       point its assessed uncertainty; MEASUREMENT for
//!                       QL-A and QL-B, INTERPRETATION for QL-C (a surface
//!                       feature correlated with records), ESTIMATE for QL-D
//! Pset_<class>TypeCommon  Reference (the asset identifier), Status
//!                       (EXISTING, NEW ...), the diameter
//! Pset_ConstructionOccurence  AssetIdentifier, InstallationDate
//! <the schema's title>  the delivery schema's attributes exactly as the
//!                       schedule wrote them (UtilityAttributes::written) -
//!                       by the schema's order and labels, and as
//!                       enumerated values of its domains, when the schema
//!                       is given
//! ```
//!
//! and each is classified: IfcClassificationReference "QL-A" .. "QL-D" in
//! AS 5488.1-2019, and the asset type code in AS 5488.2-2019 Table A.4.
//!
//! GEOMETRY IS NEVER GUESSED. A segment has an Axis in 3D - and a Body, a
//! swept disk of its size - only when both of its ends give the level of the
//! service's CENTRE: a level recorded on the centre, or on the top or invert
//! with a size to bring it to the centre. Otherwise its FootPrint is written
//! in plan and nothing else: a pipe drawn at a depth nobody measured would be
//! taken for one somebody had.

use std::collections::{BTreeMap, BTreeSet};

use katana_core::text::{lowered, parse_finite_double, parse_integer};
use katana_math::{Vec2, Vec3};
use katana_survey::subsurface::{
    self as sub, CoverResult, DeliverySchema, GradedLine, GradedSegment, LevelReference,
    PathEvidence, QualityLevel, SchemaDomain, UtilityAttributes, UtilityLine, UtilityStatus,
    UtilityVertex,
};

use crate::builder::{Builder, Property, PropertyList};
use crate::classification::{IfcClass, UtilityClass, classify_utility_point, classify_utility_run};
use crate::export::UtilityInput;
use crate::step::{step_real, step_string};
use crate::step_file::{Args, Id, typed_value};
use crate::support::trimmed;

/// The delivery schema's attributes that the schedule reader interprets
/// (utility_csv accepts them as aliases of its own columns): per service, and
/// per point. The rest arrive already by their schema names in
/// UtilityAttributes::fields and UtilityVertex::fields.
const SCHEMA_SERVICE_COLUMNS: [&str; 7] = [
    "AssetIdentifier",
    "AssetTypeCode",
    "AssetOwner",
    "AssetStatus",
    "Size",
    "Material",
    "Configuration",
];
const SCHEMA_POINT_COLUMNS: [&str; 4] = ["LocateMethod", "DepthLocation", "Depth", "QualityLevel"];

const STANDARD: &str = "AS 5488.1-2019";

fn quality_name(level: QualityLevel) -> &'static str {
    match level {
        QualityLevel::A => "Quality Level A",
        QualityLevel::B => "Quality Level B",
        QualityLevel::C => "Quality Level C",
        QualityLevel::D => "Quality Level D",
    }
}

/// IFC's PEnum_UncertaintyBasis for what a quality level rests on.
fn uncertainty_basis(level: QualityLevel) -> &'static str {
    match level {
        QualityLevel::A | QualityLevel::B => "MEASUREMENT",
        QualityLevel::C => "INTERPRETATION",
        QualityLevel::D => "ESTIMATE",
    }
}

/// PEnum_ElementStatus. A disused or abandoned service is still in the
/// ground, which is what EXISTING says; what it is used for is
/// AS5488_Service's Status.
fn element_status(status: UtilityStatus) -> &'static str {
    match status {
        UtilityStatus::InService | UtilityStatus::Disused | UtilityStatus::Abandoned => "EXISTING",
        UtilityStatus::Proposed => "NEW",
        UtilityStatus::Unknown => "NOTKNOWN",
    }
}

/// "1998/06/01", as a delivery schema writes a date, as an IfcDate
/// ("1998-06-01"); empty for anything else.
fn iso_date(text: &str) -> String {
    let text = trimmed(text).as_bytes();
    if text.len() != 10 || (text[4] != b'/' && text[4] != b'-') || text[7] != text[4] {
        return String::new();
    }
    for i in [0, 1, 2, 3, 5, 6, 8, 9] {
        if !text[i].is_ascii_digit() {
            return String::new();
        }
    }
    let mut out = text.to_vec();
    out[4] = b'-';
    out[7] = b'-';
    // Digits and two '-': ASCII.
    String::from_utf8(out).unwrap_or_default()
}

/// The level of the service's centre at `vertex`, when it can be had without
/// assuming anything: see the module's header.
fn centre_level(vertex: &UtilityVertex, diameter: f64) -> Option<f64> {
    let level = sub::service_level(vertex)?;
    match vertex.level_reference {
        LevelReference::Centre => Some(level),
        LevelReference::Top => (diameter > 0.0).then_some(level - diameter / 2.0),
        LevelReference::Invert => (diameter > 0.0).then_some(level + diameter / 2.0),
        LevelReference::Unknown => None,
    }
}

fn level_reference_name(reference: LevelReference) -> String {
    String::from(sub::to_string(reference))
}

fn plan_of(vertex: &UtilityVertex) -> Vec2 {
    Vec2::new(vertex.position.easting, vertex.position.northing)
}

fn joined(parts: &[String], separator: &str) -> String {
    let mut out = String::new();
    for part in parts {
        if !out.is_empty() {
            out.push_str(separator);
        }
        out.push_str(part);
    }
    out
}

/// The common property set IFC 4.3 defines for a segment class, if any.
fn common_property_set(entity: &str) -> &'static str {
    match entity {
        "IfcPipeSegment" => "Pset_PipeSegmentTypeCommon",
        "IfcCableSegment" => "Pset_CableSegmentTypeCommon",
        "IfcCableCarrierSegment" => "Pset_CableCarrierSegmentTypeCommon",
        "IfcDistributionChamberElement" => "Pset_DistributionChamberElementCommon",
        _ => "",
    }
}

/// The delivery schema's property set name, made from the title the schema
/// file gives itself ("Acme Utility Spec" -> "Acme_Utility_Spec"), so that
/// the set says which schema its values were written to without the export
/// knowing any client by name; "Delivery_Attributes" for a schedule written
/// to a schema that was not given.
fn delivery_property_set_name(schema: Option<&DeliverySchema>) -> String {
    let Some(schema) = schema.filter(|schema| !schema.title.is_empty()) else {
        return String::from("Delivery_Attributes");
    };
    let mut name = String::new();
    let mut gap = false;
    for &c in schema.title.as_bytes() {
        if c.is_ascii_alphanumeric() {
            if gap && !name.is_empty() {
                name.push('_');
            }
            gap = false;
            name.push(char::from(c));
        } else {
            gap = true;
        }
    }
    // PSE002: a property set of one's own must not start with "Pset".
    if name.is_empty() || lowered(&name).starts_with(b"pset") {
        name = format!("Delivery_{name}");
    }
    name
}

fn system_name(class: &UtilityClass) -> &str {
    if class.system == "USERDEFINED" {
        &class.system_object_type
    } else {
        &class.system
    }
}

struct UtilityWriter<'b, 'a> {
    b: &'b mut Builder<'a>,
    input: &'b UtilityInput,
    asset_type_classification: Id,
}

impl UtilityWriter<'_, '_> {
    fn write(&mut self) {
        let input = self.input;
        if input.lines.is_empty() {
            return;
        }
        for line in &input.lines {
            self.write_service(line);
        }
    }

    /// True when the schedule is written to the delivery schema: it names
    /// its services by AssetIdentifier, or the caller gave the schema.
    fn delivery_schedule(&self, line: &UtilityLine) -> bool {
        self.input.schema.is_some() || line.attributes.written.contains_key("AssetIdentifier")
    }

    /// The asset type codes a delivery schema's type attribute takes are
    /// AS 5488.2's (Table A.4), whichever schema the schedule was written to,
    /// so they are references into that standard; the schema itself is named
    /// by the property set its values are in.
    fn asset_type_classification(&mut self) -> Id {
        if self.asset_type_classification == 0 {
            self.asset_type_classification = self.b.classification(
                "as5488.2",
                "Standards Australia",
                "2019",
                "AS 5488.2-2019",
                "Subsurface utility information: asset types (Table A.4)",
            );
        }
        self.asset_type_classification
    }

    fn quality_reference(&mut self, level: QualityLevel) -> Id {
        quality_level_reference(self.b, level)
    }

    fn write_service(&mut self, line: &UtilityLine) {
        let key = format!("utility/{}", line.id);
        let service = &line.attributes;
        if line.vertices.is_empty() {
            self.b.warn(format!(
                "service {} has no points and is not written",
                line.id
            ));
            return;
        }

        let mut graded: Option<GradedLine> = None;
        let mut covers: Vec<CoverResult> = Vec::new();
        if line.vertices.len() >= 2 {
            match sub::grade_line(line, &self.input.grading) {
                Ok(grading) => graded = Some(grading),
                Err(error) => {
                    self.b.warn(format!(
                        "service {} is not written: {}",
                        line.id,
                        error.describe()
                    ));
                    return;
                }
            }
            if let Ok(cover) = sub::depth_of_cover(line, None, &self.input.grading) {
                covers = cover;
            }
        }

        let run = classify_utility_run(service);
        self.b
            .note_schedule_service(format!("{}/{}", line.id, sub::to_string(service.r#type)));
        let guid = self.b.guid(&key);
        let label = self.b.label(&line.id, &format!("service {}", line.id));
        let long_name = service_long_name(service);
        let system = self.b.file().add(
            "IfcDistributionSystem",
            Args::new()
                .string(&guid)
                .null()
                .string(&label)
                .string_or_null(&service.description)
                .string_or_null(&run.system_object_type)
                .string_or_null(&long_name)
                .enumeration(&run.system),
        );
        *self
            .b
            .report()
            .classes
            .entry(String::from("IfcDistributionSystem"))
            .or_insert(0) += 1;
        self.b.reference_in_site(system);
        self.b.report().services += 1;

        let mut members = Vec::new();
        // the segments or point feature: what the service's sets describe
        let mut elements = Vec::new();
        if let Some(graded) = &graded {
            for segment in &graded.segments {
                elements.push(self.write_segment(line, graded, segment, &run, &key));
            }
        } else {
            match classify_utility_point(service) {
                Some(feature) => elements.push(self.write_feature(line, &feature, &key)),
                None => self.b.warn(format!(
                    "service {} is one point and its feature names no element IFC has; it is \
                     written as its located point only",
                    line.id
                )),
            }
        }
        members.extend_from_slice(&elements);
        for v in 0..line.vertices.len() {
            members.push(self.write_located_point(line, graded.as_ref(), &covers, v, &key));
        }
        self.b.group(system, &key, &members);

        // What describes the service as a whole, on the system and on each
        // of its elements, so that whichever a person picks says it.
        let mut described = vec![system];
        described.extend_from_slice(&elements);
        let properties = self.service_properties(service, graded.as_ref());
        let set = self
            .b
            .property_set(&format!("{key}/service"), "AS5488_Service", &properties);
        self.b.defines(&format!("{key}/service"), set, &described);
        let mut reference = PropertyList::new();
        reference.identifier("Reference", &line.id);
        let set = self.b.property_set(
            &format!("{key}/system"),
            "Pset_DistributionSystemCommon",
            &reference,
        );
        self.b.defines(&format!("{key}/system"), set, &[system]);
        let mut construction = PropertyList::new();
        construction.label("AssetIdentifier", &line.id);
        if let Some(installed) = service.fields.get("UtilityInstallDate") {
            construction.date("InstallationDate", &iso_date(installed));
        }
        let set = self.b.property_set(
            &format!("{key}/construction"),
            "Pset_ConstructionOccurence",
            &construction,
        );
        self.b
            .defines(&format!("{key}/construction"), set, &elements);
        if self.delivery_schedule(line) {
            let schema = self.input.schema.as_deref();
            let delivery = self.delivery_properties(
                &service.written,
                &service.fields,
                &SCHEMA_SERVICE_COLUMNS,
            );
            let set = self.b.property_set(
                &format!("{key}/delivery"),
                &delivery_property_set_name(schema),
                &delivery,
            );
            self.b.defines(&format!("{key}/delivery"), set, &described);
            let code = match service.written.get("AssetTypeCode") {
                Some(written) => written.clone(),
                None => String::from(crate::classification::asset_type_code(service.r#type)),
            };
            if !code.is_empty() {
                let classification = self.asset_type_classification();
                let type_reference = self.b.classification_reference(
                    classification,
                    &code,
                    sub::to_string(service.r#type),
                );
                for &object in &described {
                    self.b.associate(type_reference, object);
                }
            }
        }
    }

    fn write_segment(
        &mut self,
        line: &UtilityLine,
        graded: &GradedLine,
        segment: &GradedSegment,
        run: &UtilityClass,
        key: &str,
    ) -> Id {
        let service = &line.attributes;
        let from = &line.vertices[segment.from];
        let to = &line.vertices[segment.from + 1];
        let size = service.diameter;
        let start_centre = centre_level(from, size);
        let end_centre = centre_level(to, size);
        let in_3d = start_centre.is_some() && end_centre.is_some();

        let mut representations = Vec::new();
        let layer = format!(
            "Utilities/{}/{}",
            sub::to_string(service.r#type),
            sub::to_string(segment.level)
        );
        if let (Some(start_centre), Some(end_centre)) = (start_centre, end_centre) {
            let axis = self.b.polyline_3d(
                &[
                    Vec3::new(from.position.easting, from.position.northing, start_centre),
                    Vec3::new(to.position.easting, to.position.northing, end_centre),
                ],
                false,
            );
            let axis_context = self.b.axis_context();
            representations.push(self.b.shape(axis_context, "Axis", "Curve3D", &[axis]));
            if size > 0.0 {
                let body = self.b.file().add(
                    "IfcSweptDiskSolid",
                    Args::new()
                        .reference(axis)
                        .real(size / 2.0)
                        .null()
                        .null()
                        .null(),
                );
                let body_context = self.b.body_context();
                representations.push(self.b.shape(
                    body_context,
                    "Body",
                    "AdvancedSweptSolid",
                    &[body],
                ));
            }
            self.b.report().segments_in_3d += 1;
        } else {
            let footprint = self.b.polyline(&[plan_of(from), plan_of(to)], false);
            let context = self.b.foot_print_context();
            representations.push(self.b.shape(context, "FootPrint", "Curve2D", &[footprint]));
        }
        for &representation in &representations {
            self.b.layer(&layer, representation);
        }
        let segment_key = format!("{key}/segment/{}", segment.from);
        let name = format!("{} {} to {}", line.id, from.id, to.id);
        let shape = self.b.product_shape(&representations);
        let element = self.b.product(
            &run.element,
            &segment_key,
            &name,
            &format!(
                "{STANDARD} {} segment of {}",
                sub::to_string(segment.level),
                line.id
            ),
            shape,
            &line.id,
        );
        self.b.report().service_segments += 1;
        self.b.tally(
            &format!("service {}", line.id),
            &run.element,
            system_name(run),
            &format!("a graded segment: {}", run.reason),
        );

        // ---- AS 5488: the grade -----------------------------------------------------
        let mut grade = PropertyList::new();
        grade.label_specified(
            "QualityLevel",
            sub::to_string(segment.level),
            "The AS 5488.1-2019 quality level the segment's evidence supports",
        );
        let start = &graded.vertices[segment.from];
        let end = &graded.vertices[segment.from + 1];
        if let (Some(from_claimed), Some(to_claimed)) = (from.claimed, to.claimed) {
            let claimed = from_claimed.min(to_claimed);
            grade.label_specified(
                "QualityLevelClaimed",
                sub::to_string(claimed),
                "The weaker of the levels the deliverable claims for its two ends",
            );
            grade.boolean("ClaimSupported", Some(claimed <= segment.level));
        }
        grade.text("LimitedBy", &segment.limited_by);
        let evidence = if line.path_evidence.is_empty() {
            PathEvidence::Detected
        } else {
            line.path_evidence[segment.from]
        };
        grade.label_specified(
            "PathEvidence",
            sub::to_string(evidence),
            "What was observed between the two points",
        );
        grade
            .identifier("StartPoint", &from.id)
            .identifier("EndPoint", &to.id);
        grade.length("PlanLength", Some(segment.length));
        grade.label(
            "StartLevelReference",
            &if sub::service_level(from).is_some() {
                level_reference_name(from.level_reference)
            } else {
                String::new()
            },
        );
        grade.label(
            "EndLevelReference",
            &if sub::service_level(to).is_some() {
                level_reference_name(to.level_reference)
            } else {
                String::new()
            },
        );
        grade
            .length("StartCentreLevel", start_centre)
            .length("EndCentreLevel", end_centre);
        if sub::has_vertical_measurement(from) && sub::has_vertical_measurement(to) {
            grade.boolean(
                "LevelsQualified",
                Some(start.classification.level_qualified && end.classification.level_qualified),
            );
        }
        grade.boolean("DrawnIn3D", Some(in_3d));
        grade.label("Standard", STANDARD);
        let set = self.b.property_set(
            &format!("{segment_key}/grade"),
            "AS5488_QualityLevel",
            &grade,
        );
        self.b
            .defines(&format!("{segment_key}/grade"), set, &[element]);

        // ---- IFC's uncertainty: the tolerance the level certifies ---------------
        let tolerance = self.input.grading.tolerances.of(segment.level);
        let mut uncertainty = PropertyList::new();
        uncertainty.enumerated("UncertaintyBasis", uncertainty_basis(segment.level), 0, "");
        uncertainty.text(
            "UncertaintyDescription",
            &format!(
                "{STANDARD} {}{}",
                sub::to_string(segment.level),
                if tolerance.horizontal.is_some() {
                    ": the tolerance of the quality level"
                } else {
                    ": the quality level states no tolerance"
                }
            ),
        );
        uncertainty.positive_length("HorizontalUncertainty", tolerance.horizontal);
        if in_3d && start.classification.level_qualified && end.classification.level_qualified {
            uncertainty.positive_length("VerticalUncertainty", tolerance.vertical);
        }
        let set = self.b.property_set(
            &format!("{segment_key}/uncertainty"),
            "Pset_Uncertainty",
            &uncertainty,
        );
        self.b
            .defines(&format!("{segment_key}/uncertainty"), set, &[element]);

        // ---- the class's own common set -------------------------------------------
        let common = common_property_set(&run.element.entity);
        if !common.is_empty() {
            let mut list = PropertyList::new();
            list.identifier("Reference", &line.id);
            list.enumerated("Status", element_status(service.status), 0, "");
            if run.element.entity == "IfcPipeSegment" && size > 0.0 {
                list.positive_length(
                    if service.diameter_is_inside {
                        "InnerDiameter"
                    } else {
                        "OuterDiameter"
                    },
                    Some(size),
                );
                if let (Some(start_centre), Some(end_centre)) = (start_centre, end_centre) {
                    let rise = end_centre - start_centre;
                    list.positive_length(
                        "Length",
                        Some((segment.length * segment.length + rise * rise).sqrt()),
                    );
                }
            }
            let set = self
                .b
                .property_set(&format!("{segment_key}/common"), common, &list);
            self.b
                .defines(&format!("{segment_key}/common"), set, &[element]);
        }
        if run.element.entity == "IfcPipeSegment"
            && segment.length > 0.0
            && let (Some(start_centre), Some(end_centre)) = (start_centre, end_centre)
        {
            let mut occurrence = PropertyList::new();
            occurrence.positive_ratio(
                "Gradient",
                Some((end_centre - start_centre).abs() / segment.length),
            );
            if service.diameter_is_inside && size > 0.0 {
                // The inside bottom of the bore, at the start: centre less
                // half the inside size is exact for an inside size.
                occurrence.length("InvertElevation", Some(start_centre - size / 2.0));
            }
            let set = self.b.property_set(
                &format!("{segment_key}/occurrence"),
                "Pset_PipeSegmentOccurrence",
                &occurrence,
            );
            self.b
                .defines(&format!("{segment_key}/occurrence"), set, &[element]);
        }
        let reference = self.quality_reference(segment.level);
        self.b.associate(reference, element);
        element
    }

    fn write_feature(&mut self, line: &UtilityLine, feature: &UtilityClass, key: &str) -> Id {
        let vertex = &line.vertices[0];
        let mut items = vec![self.b.point(plan_of(vertex))];
        if line.attributes.diameter > 0.0 {
            let centre = self.b.point(plan_of(vertex));
            let placement = self.b.placement2(centre, 0.0);
            items.push(
                self.b.file().add(
                    "IfcCircle",
                    Args::new()
                        .reference(placement)
                        .real(line.attributes.diameter / 2.0),
                ),
            );
        }
        let set = self
            .b
            .file()
            .add("IfcGeometricCurveSet", Args::new().refs(&items));
        let context = self.b.foot_print_context();
        let representation = self
            .b
            .shape(context, "FootPrint", "GeometricCurveSet", &[set]);
        let classification = sub::classify(&vertex.evidence, &self.input.grading.tolerances);
        self.b.layer(
            &format!(
                "Utilities/{}/{}",
                sub::to_string(line.attributes.r#type),
                sub::to_string(classification.level)
            ),
            representation,
        );
        let shape = self.b.product_shape(&[representation]);
        let element = self.b.product(
            &feature.element,
            &format!("{key}/feature"),
            &line.id,
            &feature.reason,
            shape,
            &line.id,
        );
        self.b.tally(
            &format!("service {}", line.id),
            &feature.element,
            system_name(feature),
            &format!("a service at one point: {}", feature.reason),
        );
        let common = common_property_set(&feature.element.entity);
        if !common.is_empty() {
            let mut list = PropertyList::new();
            list.identifier("Reference", &line.id);
            list.enumerated("Status", element_status(line.attributes.status), 0, "");
            let set = self
                .b
                .property_set(&format!("{key}/feature/common"), common, &list);
            self.b
                .defines(&format!("{key}/feature/common"), set, &[element]);
        }
        let reference = self.quality_reference(classification.level);
        self.b.associate(reference, element);
        element
    }

    fn write_located_point(
        &mut self,
        line: &UtilityLine,
        graded: Option<&GradedLine>,
        covers: &[CoverResult],
        index: usize,
        key: &str,
    ) -> Id {
        let vertex = &line.vertices[index];
        let classification = match graded {
            Some(graded) => graded.vertices[index].classification.clone(),
            None => sub::classify(&vertex.evidence, &self.input.grading.tolerances),
        };
        let level = sub::service_level(vertex);
        let point = match level {
            Some(level) => self.b.point_3d(Vec3::new(
                vertex.position.easting,
                vertex.position.northing,
                level,
            )),
            None => self.b.point(plan_of(vertex)),
        };
        let context = self.b.annotation_context();
        let representation = self.b.shape(context, "Annotation", "Point", &[point]);
        self.b.layer(
            &format!(
                "Utilities/{}/points",
                sub::to_string(line.attributes.r#type)
            ),
            representation,
        );
        let point_key = format!("{key}/point/{}", vertex.id);
        let survey = IfcClass::new("IfcAnnotation", "SURVEY", "");
        let shape = self.b.product_shape(&[representation]);
        let annotation = self.b.product(
            &survey,
            &point_key,
            &vertex.id,
            &format!("Located by {}", sub::to_string(vertex.evidence.method)),
            shape,
            "",
        );
        self.b.report().located_points += 1;
        // In the service's system, as the file groups it.
        let run = classify_utility_run(&line.attributes);
        self.b.tally(
            &format!("service {}", line.id),
            &survey,
            system_name(&run),
            "a located point",
        );

        let mut located = PropertyList::new();
        located.label("LocateMethod", sub::to_string(vertex.evidence.method));
        located.label_specified(
            "QualityLevel",
            sub::to_string(classification.level),
            "The AS 5488.1-2019 quality level the point's evidence supports",
        );
        if let Some(claimed) = vertex.claimed {
            located.label("QualityLevelClaimed", sub::to_string(claimed));
            located.boolean("ClaimSupported", Some(claimed <= classification.level));
        }
        if let Some(graded) = graded {
            located.text("OverClaim", &graded.vertices[index].over_claim);
        }
        located.text("Reasons", &joined(&classification.reasons, "; "));
        if vertex.evidence.has_level {
            located.boolean("LevelQualified", Some(classification.level_qualified));
        }
        located.label(
            "LevelReference",
            &if level.is_some() {
                level_reference_name(vertex.level_reference)
            } else {
                String::new()
            },
        );
        located.length("Level", vertex.level);
        located.length("SurfaceLevel", vertex.surface_level);
        located.length("Depth", vertex.depth);
        located.length("TopLevel", sub::top_level(vertex, line.attributes.diameter));
        if let Some(cover) = covers.get(index) {
            located.length("DepthOfCover", cover.cover);
            located.text("CoverNote", &cover.note);
        }
        located.identifier("Verifies", &vertex.verifies);
        located.label("Standard", STANDARD);
        let set = self.b.property_set(
            &format!("{point_key}/located"),
            "AS5488_LocatedPoint",
            &located,
        );
        self.b
            .defines(&format!("{point_key}/located"), set, &[annotation]);

        let mut uncertainty = PropertyList::new();
        uncertainty.enumerated(
            "UncertaintyBasis",
            uncertainty_basis(classification.level),
            0,
            "",
        );
        if vertex.evidence.horizontal_uncertainty.is_some()
            || vertex.evidence.vertical_uncertainty.is_some()
        {
            uncertainty.text(
                "UncertaintyDescription",
                "The assessed uncertainty of the located position",
            );
        }
        uncertainty.positive_length(
            "HorizontalUncertainty",
            vertex.evidence.horizontal_uncertainty,
        );
        uncertainty.positive_length("VerticalUncertainty", vertex.evidence.vertical_uncertainty);
        let set = self.b.property_set(
            &format!("{point_key}/uncertainty"),
            "Pset_Uncertainty",
            &uncertainty,
        );
        self.b
            .defines(&format!("{point_key}/uncertainty"), set, &[annotation]);

        if self.delivery_schedule(line) {
            let schema = self.input.schema.as_deref();
            let delivery =
                self.delivery_properties(&vertex.written, &vertex.fields, &SCHEMA_POINT_COLUMNS);
            let set = self.b.property_set(
                &format!("{point_key}/delivery"),
                &delivery_property_set_name(schema),
                &delivery,
            );
            self.b
                .defines(&format!("{point_key}/delivery"), set, &[annotation]);
        }
        let reference = self.quality_reference(classification.level);
        self.b.associate(reference, annotation);
        annotation
    }

    fn service_properties(
        &self,
        service: &UtilityAttributes,
        graded: Option<&GradedLine>,
    ) -> PropertyList {
        let mut list = PropertyList::new();
        list.label("UtilityType", sub::to_string(service.r#type));
        list.label("Owner", &service.owner);
        list.label("Material", &service.material);
        list.positive_length("Size", Some(service.diameter));
        if service.diameter > 0.0 {
            list.boolean("SizeIsInside", Some(service.diameter_is_inside));
        }
        list.label("Configuration", &service.configuration);
        list.label(
            "Status",
            if service.status == UtilityStatus::Unknown {
                ""
            } else {
                sub::to_string(service.status)
            },
        );
        list.text("Description", &service.description);
        if let Some(graded) = graded {
            list.length("PlanLength", Some(graded.length()));
            for level in [
                QualityLevel::A,
                QualityLevel::B,
                QualityLevel::C,
                QualityLevel::D,
            ] {
                let length = graded.length_at[level as usize];
                if length > 0.0 {
                    // "QL-A"[3]: the level's letter.
                    let letter = &sub::to_string(level)[3..4];
                    list.length(&format!("LengthAtQL{letter}"), Some(length));
                }
            }
        }
        list.label("SourceSchedule", &self.input.source_name);
        list.label("Standard", STANDARD);
        list
    }

    /// The delivery schema's attributes: those the reader interpreted, as
    /// written, and those it carried by name.
    fn delivery_properties(
        &mut self,
        written: &BTreeMap<String, String>,
        fields: &BTreeMap<String, String>,
        interpreted: &[&str],
    ) -> PropertyList {
        let mut values: BTreeMap<String, String> = BTreeMap::new();
        for &column in interpreted {
            if let Some((name, value)) = written.get_key_value(column) {
                values.entry(name.clone()).or_insert_with(|| value.clone());
            }
        }
        for (name, value) in fields {
            values.entry(name.clone()).or_insert_with(|| value.clone());
        }
        let mut list = PropertyList::new();
        let Some(schema) = self.input.schema.clone() else {
            for (name, value) in &values {
                list.label(name, value);
            }
            return list;
        };
        // In the schema's order, described by its labels, enumerated where
        // it lists the values. A value its list does not hold (the check
        // reports it) is written as the text it is: an enumerated value
        // outside its enumeration is a schema error in the file (WR21 of
        // IfcPropertyEnumeratedValue), and the value must not be lost.
        for field in &schema.fields {
            let Some(value) = values
                .get(&field.attribute)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            if let Some(domain) = schema.domains.get(&field.attribute) {
                let listed = domain
                    .values
                    .iter()
                    .any(|candidate| &candidate.value == value);
                if listed {
                    let enumeration = self.enumeration_of(&field.attribute, domain);
                    list.enumerated(&field.attribute, value, enumeration, &field.label);
                    continue;
                }
            }
            if field.r#type.starts_with("Real") {
                if let Some(number) = parse_finite_double(value) {
                    list.add(Property::new(
                        &field.attribute,
                        typed_value("IfcReal", &step_real(number)),
                        &field.label,
                    ));
                    continue;
                }
            } else if field.r#type.starts_with("Integer")
                && let Some(number) = parse_integer(value)
            {
                list.add(Property::new(
                    &field.attribute,
                    typed_value("IfcInteger", &format!("{number}")),
                    &field.label,
                ));
                continue;
            }
            list.label_specified(&field.attribute, value, &field.label);
        }
        list
    }

    fn enumeration_of(&mut self, attribute: &str, domain: &SchemaDomain) -> Id {
        let mut encoded = Vec::new();
        let mut seen = BTreeSet::new();
        for value in &domain.values {
            if seen.insert(value.value.clone()) {
                encoded.push(typed_value("IfcLabel", &step_string(&value.value)));
            }
        }
        self.b.enumeration(attribute, &encoded)
    }
}

pub(crate) fn export_utilities(builder: &mut Builder<'_>, utilities: &UtilityInput) {
    UtilityWriter {
        b: builder,
        input: utilities,
        asset_type_classification: 0,
    }
    .write();
}

/// A service's system LongName - its owner, when known, and its type
/// ("WaterCo water") - the same from the schedule's export and a plan drawn
/// from it.
pub(crate) fn service_long_name(service: &UtilityAttributes) -> String {
    let mut parts = Vec::new();
    if !service.owner.is_empty() {
        parts.push(service.owner.clone());
    }
    parts.push(String::from(sub::to_string(service.r#type)));
    joined(&parts, " ")
}

/// The reference "QL-A" .. "QL-D" into AS 5488.1-2019, the classification
/// written once whichever part asks first - the schedule's services or a
/// services plan drawn from one (drawing.rs) - so that a file holding both
/// classifies them in one standard.
pub(crate) fn quality_level_reference(builder: &mut Builder<'_>, level: QualityLevel) -> Id {
    let standard = builder.classification(
        "as5488",
        "Standards Australia",
        "2019",
        STANDARD,
        "Classification of subsurface utility information: quality levels A to D",
    );
    builder.classification_reference(standard, sub::to_string(level), quality_name(level))
}
