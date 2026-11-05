use crate::chording::sagitta_chord_count;
use crate::scalar::max;
use crate::{Arc2, Point2, Vec2};

const NODES: [f64; 5] = [
    0.1488743389816312,
    0.4333953941292472,
    0.6794095682990244,
    0.8650633666889845,
    0.9739065285171717,
];
#[allow(
    clippy::excessive_precision,
    reason = "Preserve the quadrature weight literals exactly as written in the C++ oracle."
)]
const WEIGHTS: [f64; 5] = [
    0.2955242247147529,
    0.2692667193099963,
    0.2190863625159820,
    0.1494513491505806,
    0.0666713443086881,
];
const MAX_PANEL_TURN: f64 = 0.5;
const MAX_PANELS: usize = 4096;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spiral2 {
    pub start: Point2,
    pub start_direction: f64,
    pub start_curvature: f64,
    pub end_curvature: f64,
    pub length: f64,
}
impl Spiral2 {
    pub fn from_straight_to_radius(
        start: Point2,
        direction: f64,
        radius: f64,
        length: f64,
    ) -> Self {
        Self {
            start,
            start_direction: direction,
            start_curvature: 0.0,
            end_curvature: if radius != 0.0 { 1.0 / radius } else { 0.0 },
            length,
        }
    }
    pub fn flatness(&self) -> f64 {
        let rate = (self.end_curvature - self.start_curvature).abs();
        if !(rate > 0.0) || !(self.length > 0.0) {
            f64::INFINITY
        } else {
            (self.length / rate).sqrt()
        }
    }
    pub fn curvature_at(&self, distance: f64) -> f64 {
        if !(self.length > 0.0) {
            self.start_curvature
        } else {
            self.start_curvature
                + (self.end_curvature - self.start_curvature) * (distance / self.length)
        }
    }
    pub fn direction_at(&self, distance: f64) -> f64 {
        if !(self.length > 0.0) {
            return self.start_direction;
        }
        self.start_direction
            + self.start_curvature * distance
            + (self.end_curvature - self.start_curvature) * distance * distance
                / (2.0 * self.length)
    }
    pub fn total_turn(&self) -> f64 {
        self.direction_at(self.length) - self.start_direction
    }
    pub fn point_at(&self, distance: f64) -> Point2 {
        if distance == 0.0 || !distance.is_finite() {
            return self.start;
        }
        let far_curvature = self.curvature_at(distance);
        let worst_curvature = max(self.start_curvature.abs(), far_curvature.abs());
        let turn_bound = worst_curvature * distance.abs();
        let panels = (turn_bound / MAX_PANEL_TURN)
            .ceil()
            .clamp(1.0, MAX_PANELS as f64) as usize;
        let panel_length = distance / panels as f64;
        let half_panel = 0.5 * panel_length;
        let mut east = 0.0;
        let mut north = 0.0;
        for panel in 0..panels {
            let center = (panel as f64 + 0.5) * panel_length;
            for index in 0..NODES.len() {
                let offset = NODES[index] * half_panel;
                let plus = self.direction_at(center + offset);
                let minus = self.direction_at(center - offset);
                east += WEIGHTS[index] * (plus.cos() + minus.cos());
                north += WEIGHTS[index] * (plus.sin() + minus.sin());
            }
        }
        Point2::new(
            self.start.x + east * half_panel,
            self.start.y + north * half_panel,
        )
    }
    pub fn tangent_at(&self, distance: f64) -> Vec2 {
        let angle = self.direction_at(distance);
        Vec2::new(angle.cos(), angle.sin())
    }
    pub fn end_point(&self) -> Point2 {
        self.point_at(self.length)
    }
    pub fn end_direction(&self) -> f64 {
        self.direction_at(self.length)
    }
    pub fn radius_at(&self, distance: f64) -> f64 {
        let curvature = self.curvature_at(distance);
        if curvature != 0.0 {
            1.0 / curvature
        } else {
            f64::INFINITY
        }
    }
    pub fn is_degenerate(&self) -> bool {
        self.start_curvature == self.end_curvature
    }
    pub fn as_arc(&self) -> Option<Arc2> {
        if !self.is_degenerate() || self.start_curvature == 0.0 || !(self.length > 0.0) {
            return None;
        }
        let radius = 1.0 / self.start_curvature;
        let center = Point2::new(
            self.start.x - self.start_direction.sin() * radius,
            self.start.y + self.start_direction.cos() * radius,
        );
        Some(Arc2::new(
            center,
            radius.abs(),
            (self.start.y - center.y).atan2(self.start.x - center.x),
            self.start_curvature * self.length,
        ))
    }
    pub fn chord_count_for(&self, epsilon: f64) -> usize {
        let worst_curvature = max(self.start_curvature.abs(), self.end_curvature.abs());
        if !(worst_curvature > 0.0) || !(self.length > 0.0) {
            return 2;
        }
        sagitta_chord_count(
            1.0 / worst_curvature,
            self.length * worst_curvature,
            epsilon,
        ) + 1
    }
}
