//! SurveyNetwork: the container that owns points, control, setups and
//! observations of one survey.
//!
//! Invariants, enforced by every mutator (a failed call leaves the network
//! unchanged):
//!   * point ids are unique and non-empty, coordinates are finite;
//!   * every observation, control record and setup refers to existing points;
//!   * every stored observation satisfies validate_observation(), with azimuths
//!     and horizontal angles normalised into [0, 2*pi).
//!
//! Iteration order is insertion order, so output derived from a network is
//! deterministic. Observations are addressed by index; removing one shifts the
//! indices above it.
//!
//! Threading: a plain value. Concurrent shared access is safe, any mutation
//! requires exclusive access.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

use std::collections::BTreeMap;

use katana_core::{Error, ErrorCode, Result, Status};

use crate::data_model::{
    ControlComponent, ControlConstraint, ControlPoint, Observation, Station, SurveyPoint,
    normalized_observation, observation_kind_name, referenced_points, validate_observation,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyNetwork {
    points: Vec<SurveyPoint>,
    // id -> position in points
    index: BTreeMap<String, usize>,
    control: Vec<ControlPoint>,
    stations: Vec<Station>,
    observations: Vec<Observation>,
}

fn check_point_values(point: &SurveyPoint) -> Status {
    if point.id.is_empty() {
        return Err(Error::new(ErrorCode::InvalidArgument, "point id is empty"));
    }
    if !point.northing.is_finite()
        || !point.easting.is_finite()
        || point
            .elevation
            .is_some_and(|elevation| !elevation.is_finite())
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "point coordinates are not finite",
        )
        .with_context(format!("point {}", point.id)));
    }
    Ok(())
}

fn check_control_component(component: &ControlComponent, point_id: &str, name: &str) -> Status {
    if component.constraint != ControlConstraint::Weighted {
        return Ok(());
    }
    if !component.sigma.is_finite() || !(component.sigma > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("weighted control needs a positive finite sigma for its {name}"),
        )
        .with_context(format!("point {point_id}")));
    }
    Ok(())
}

impl SurveyNetwork {
    // ---- points ----

    /// AlreadyExists for a duplicate id, InvalidArgument for an empty id or
    /// non-finite coordinates.
    pub fn add_point(&mut self, point: SurveyPoint) -> Status {
        check_point_values(&point)?;
        if self.contains_point(&point.id) {
            return Err(Error::new(
                ErrorCode::AlreadyExists,
                "a point with this id already exists",
            )
            .with_context(format!("point {}", point.id)));
        }
        self.index.insert(point.id.clone(), self.points.len());
        self.points.push(point);
        Ok(())
    }

    /// Replaces the point with the same id. NotFound / InvalidArgument.
    pub fn update_point(&mut self, point: SurveyPoint) -> Status {
        check_point_values(&point)?;
        let Some(index) = self.point_index(&point.id) else {
            return Err(Error::new(ErrorCode::NotFound, "no point with this id")
                .with_context(format!("point {}", point.id)));
        };
        self.points[index] = point;
        Ok(())
    }

    /// InvalidState while an observation, control record or setup still refers
    /// to the point; NotFound when absent.
    pub fn remove_point(&mut self, id: &str) -> Status {
        let Some(index) = self.point_index(id) else {
            return Err(Error::new(ErrorCode::NotFound, "no point with this id")
                .with_context(format!("point {id}")));
        };
        if self.is_referenced(id) {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "point is still referenced by an observation, control record or setup",
            )
            .with_context(format!("point {id}")));
        }
        self.points.remove(index);
        self.rebuild_index();
        Ok(())
    }

    pub fn contains_point(&self, id: &str) -> bool {
        self.index.contains_key(id)
    }

    pub fn point_index(&self, id: &str) -> Option<usize> {
        self.index.get(id).copied()
    }

    pub fn point(&self, id: &str) -> Result<SurveyPoint> {
        let Some(index) = self.point_index(id) else {
            return Err(Error::new(ErrorCode::NotFound, "no point with this id")
                .with_context(format!("point {id}")));
        };
        Ok(self.points[index].clone())
    }

    pub fn points(&self) -> &[SurveyPoint] {
        &self.points
    }

    // ---- control ----

    /// NotFound for an unknown point, AlreadyExists when the point already has a
    /// control record, InvalidArgument when no component is constrained or a
    /// weighted component lacks a positive finite sigma.
    pub fn add_control_point(&mut self, control: ControlPoint) -> Status {
        if !self.contains_point(&control.point_id) {
            return Err(
                Error::new(ErrorCode::NotFound, "control refers to an unknown point")
                    .with_context(format!("point {}", control.point_id)),
            );
        }
        if self.control_for(&control.point_id).is_some() {
            return Err(Error::new(
                ErrorCode::AlreadyExists,
                "the point already has a control record",
            )
            .with_context(format!("point {}", control.point_id)));
        }
        if control.northing.constraint == ControlConstraint::Free
            && control.easting.constraint == ControlConstraint::Free
            && control.elevation.constraint == ControlConstraint::Free
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a control record must constrain at least one component",
            )
            .with_context(format!("point {}", control.point_id)));
        }
        check_control_component(&control.northing, &control.point_id, "northing")?;
        check_control_component(&control.easting, &control.point_id, "easting")?;
        check_control_component(&control.elevation, &control.point_id, "elevation")?;
        self.control.push(control);
        Ok(())
    }

    pub fn remove_control_point(&mut self, point_id: &str) -> Status {
        let Some(found) = self
            .control
            .iter()
            .position(|control| control.point_id == point_id)
        else {
            return Err(
                Error::new(ErrorCode::NotFound, "the point has no control record")
                    .with_context(format!("point {point_id}")),
            );
        };
        self.control.remove(found);
        Ok(())
    }

    pub fn control_for(&self, point_id: &str) -> Option<ControlPoint> {
        self.control
            .iter()
            .find(|control| control.point_id == point_id)
            .cloned()
    }

    pub fn control_points(&self) -> &[ControlPoint] {
        &self.control
    }

    // ---- setups ----

    pub fn add_station(&mut self, station: Station) -> Status {
        if station.id.is_empty() {
            return Err(Error::new(ErrorCode::InvalidArgument, "setup id is empty"));
        }
        if !station.instrument_height.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "instrument height is not finite",
            )
            .with_context(format!("setup {}", station.id)));
        }
        if !self.contains_point(&station.point_id) {
            return Err(
                Error::new(ErrorCode::NotFound, "setup occupies an unknown point").with_context(
                    format!("setup {} at point {}", station.id, station.point_id),
                ),
            );
        }
        if self.stations.iter().any(|s| s.id == station.id) {
            return Err(Error::new(
                ErrorCode::AlreadyExists,
                "a setup with this id already exists",
            )
            .with_context(format!("setup {}", station.id)));
        }
        self.stations.push(station);
        Ok(())
    }

    pub fn remove_station(&mut self, station_id: &str) -> Status {
        let Some(found) = self.stations.iter().position(|s| s.id == station_id) else {
            return Err(Error::new(ErrorCode::NotFound, "no setup with this id")
                .with_context(format!("setup {station_id}")));
        };
        self.stations.remove(found);
        Ok(())
    }

    pub fn stations(&self) -> &[Station] {
        &self.stations
    }

    // ---- observations ----

    /// Returns the index of the stored observation. InvalidSurveyObservation for
    /// bad values, NotFound for an unknown station. Takes any observation kind,
    /// as the C++ converts each to the variant.
    pub fn add_observation(&mut self, observation: impl Into<Observation>) -> Result<usize> {
        let observation = observation.into();
        validate_observation(&observation)?;
        for id in referenced_points(&observation) {
            if !self.contains_point(&id) {
                return Err(Error::new(
                    ErrorCode::NotFound,
                    "observation refers to an unknown station",
                )
                .with_context(format!(
                    "{}, station {id}",
                    observation_kind_name(&observation)
                )));
            }
        }
        self.observations.push(normalized_observation(observation));
        Ok(self.observations.len() - 1)
    }

    pub fn remove_observation(&mut self, index: usize) -> Status {
        if index >= self.observations.len() {
            return Err(
                Error::new(ErrorCode::NotFound, "observation index out of range")
                    .with_context(format!("index {index}, count {}", self.observations.len())),
            );
        }
        self.observations.remove(index);
        Ok(())
    }

    pub fn observations(&self) -> &[Observation] {
        &self.observations
    }

    // ---- helpers ----

    fn is_referenced(&self, point_id: &str) -> bool {
        if self.control_for(point_id).is_some() {
            return true;
        }
        if self.stations.iter().any(|s| s.point_id == point_id) {
            return true;
        }
        self.observations
            .iter()
            .any(|o| referenced_points(o).iter().any(|id| id == point_id))
    }

    fn rebuild_index(&mut self) {
        self.index.clear();
        for (i, point) in self.points.iter().enumerate() {
            self.index.insert(point.id.clone(), i);
        }
    }
}
