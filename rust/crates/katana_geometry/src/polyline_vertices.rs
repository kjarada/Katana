use crate::chording::chord_arc;
use crate::curves2d::{bulge_from_sweep, bulge_through};
use crate::{CurvePolyline2, CurveSegment, CurveVertex, Point2, Segment2, Vec2};
use katana_core::error::{Error, ErrorCode, Result, Status};
use katana_core::text::format_exact_real;
use katana_math::cpp::{max, min};
use katana_math::numerics::{PI, tolerance};

pub type PolylineResult = Result<CurvePolyline2>;
fn invalid(why: impl Into<String>) -> Error {
    Error::new(ErrorCode::InvalidArgument, why)
}
fn degenerate(why: impl Into<String>) -> Error {
    Error::new(ErrorCode::InvalidGeometry, why)
}
fn coincide(first: Point2, second: Point2) -> bool {
    first.distance_to(second) <= tolerance::GEOMETRIC
}
fn straight(bulge: f64) -> bool {
    bulge.abs() <= 1.0e-12
}
fn lerp_height(first: Option<f64>, second: Option<f64>, parameter: f64) -> Option<f64> {
    first
        .zip(second)
        .map(|(first, second)| first + (second - first) * parameter)
}
fn check_vertex(polyline: &CurvePolyline2, index: usize) -> Status {
    if polyline.vertices.is_empty() {
        return Err(invalid("the polyline has no vertices"));
    }
    if index >= polyline.vertices.len() {
        return Err(invalid(format!(
            "there is no vertex {index}; the vertices are 0 to {}",
            polyline.vertices.len() - 1
        )));
    }
    Ok(())
}
fn check_segment(polyline: &CurvePolyline2, segment: usize) -> Status {
    let count = polyline.segment_count();
    if segment >= count {
        return Err(invalid(if count == 0 {
            "the polyline has no segments".to_owned()
        } else {
            format!(
                "there is no segment {segment}; the segments are 0 to {}",
                count - 1
            )
        }));
    }
    Ok(())
}
fn previous_vertex(polyline: &CurvePolyline2, index: usize) -> usize {
    if index == 0 {
        polyline.vertices.len() - 1
    } else {
        index - 1
    }
}
fn fraction_along(piece: CurveSegment, point: Point2) -> f64 {
    match piece {
        CurveSegment::Segment(line) => line.parameter_of(point),
        CurveSegment::Arc(arc) => arc.parameter_of_angle((point - arc.center).angle()),
    }
}
pub fn minimum_vertices(polyline: &CurvePolyline2) -> usize {
    if polyline.closed { 3 } else { 2 }
}
pub fn nearest_vertex(polyline: &CurvePolyline2, point: Point2) -> Option<usize> {
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    for (index, vertex) in polyline.vertices.iter().enumerate() {
        let distance = vertex.position.distance_to(point);
        if distance < best_distance {
            best_distance = distance;
            best = Some(index);
        }
    }
    best
}
pub fn nearest_segment(polyline: &CurvePolyline2, point: Point2) -> Option<usize> {
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    for index in 0..polyline.segment_count() {
        let distance = match polyline.segment(index) {
            CurveSegment::Segment(piece) => piece.distance_to(point),
            CurveSegment::Arc(piece) => piece.distance_to(point),
        };
        if distance < best_distance {
            best_distance = distance;
            best = Some(index);
        }
    }
    best
}
pub fn insert_vertex(
    polyline: &CurvePolyline2,
    segment: usize,
    at: Point2,
    height: Option<f64>,
) -> PolylineResult {
    check_segment(polyline, segment)?;
    if !at.is_finite() {
        return Err(invalid("the new vertex is not a finite point"));
    }
    let mut output = polyline.clone();
    let first = polyline.vertices[segment];
    let last = polyline.vertices[polyline.segment_end(segment)];
    let piece = polyline.segment(segment);
    let mut added = CurveVertex::default();
    let parameter;
    if let CurveSegment::Arc(arc) = piece {
        added.position = arc.closest_point(at);
        parameter = fraction_along(piece, added.position);
        output.vertices[segment].bulge = bulge_from_sweep(arc.sweep * parameter);
        added.bulge = bulge_from_sweep(arc.sweep * (1.0 - parameter));
    } else {
        added.position = at;
        parameter = fraction_along(piece, at).clamp(0.0, 1.0);
    }
    if coincide(added.position, first.position) || coincide(added.position, last.position) {
        return Err(invalid(
            "the point is on a vertex already; a new vertex must be between two",
        ));
    }
    added.height = height.or_else(|| lerp_height(first.height, last.height, parameter));
    output.vertices.insert(segment + 1, added);
    Ok(output)
}
pub fn delete_vertex(polyline: &CurvePolyline2, index: usize) -> PolylineResult {
    check_vertex(polyline, index)?;
    if polyline.vertices.len() <= minimum_vertices(polyline) {
        return Err(degenerate(format!(
            "a {}polyline needs at least {} vertices; this one has no vertex to spare",
            if polyline.closed { "closed " } else { "" },
            minimum_vertices(polyline)
        )));
    }
    let mut output = polyline.clone();
    if polyline.closed || (index > 0 && index + 1 < polyline.vertices.len()) {
        output.vertices[previous_vertex(polyline, index)].bulge = 0.0;
    }
    output.vertices.remove(index);
    if !output.closed {
        output.vertices.last_mut().unwrap().bulge = 0.0;
    }
    Ok(output)
}
pub fn delete_vertices(polyline: &CurvePolyline2, mut indices: Vec<usize>) -> PolylineResult {
    indices.sort();
    indices.dedup();
    if indices.is_empty() {
        return Err(invalid("no vertex was given to delete"));
    }
    for &index in &indices {
        check_vertex(polyline, index)?;
    }
    if polyline.vertices.len() < indices.len() + minimum_vertices(polyline) {
        return Err(degenerate(format!(
            "deleting {} vertices would leave {}; a polyline needs at least {}",
            indices.len(),
            polyline.vertices.len() - indices.len(),
            minimum_vertices(polyline)
        )));
    }
    let mut output = polyline.clone();
    for &index in indices.iter().rev() {
        output = delete_vertex(&output, index)?;
    }
    Ok(output)
}
pub fn move_vertex(polyline: &CurvePolyline2, index: usize, to: Point2) -> PolylineResult {
    check_vertex(polyline, index)?;
    if !to.is_finite() {
        return Err(invalid(
            "the vertex cannot be moved to a point that is not finite",
        ));
    }
    let mut output = polyline.clone();
    output.vertices[index].position = to;
    Ok(output)
}
pub fn set_vertex_height(
    polyline: &CurvePolyline2,
    index: usize,
    height: Option<f64>,
) -> PolylineResult {
    check_vertex(polyline, index)?;
    if height.is_some_and(|height| !height.is_finite()) {
        return Err(invalid("a height must be a finite number"));
    }
    let mut output = polyline.clone();
    output.vertices[index].height = height;
    Ok(output)
}
pub fn set_segment_bulge(polyline: &CurvePolyline2, segment: usize, bulge: f64) -> PolylineResult {
    check_segment(polyline, segment)?;
    if !bulge.is_finite() {
        return Err(invalid("a bulge must be a finite number"));
    }
    let mut output = polyline.clone();
    output.vertices[segment].bulge = bulge;
    Ok(output)
}
pub fn segment_to_arc(
    polyline: &CurvePolyline2,
    segment: usize,
    through: Point2,
) -> PolylineResult {
    check_segment(polyline, segment)?;
    let first = polyline.vertices[segment].position;
    let last = polyline.vertices[polyline.segment_end(segment)].position;
    let bulge = bulge_through(first, through, last);
    if straight(bulge) {
        return Err(invalid(
            "the point is in line with the segment's ends; an arc needs a point off the line",
        ));
    }
    set_segment_bulge(polyline, segment, bulge)
}
pub fn segment_to_line(polyline: &CurvePolyline2, segment: usize) -> PolylineResult {
    check_segment(polyline, segment)?;
    if straight(polyline.vertices[segment].bulge) {
        return Err(invalid(format!("segment {segment} is already straight")));
    }
    set_segment_bulge(polyline, segment, 0.0)
}
pub fn move_segment(polyline: &CurvePolyline2, segment: usize, delta: Vec2) -> PolylineResult {
    check_segment(polyline, segment)?;
    if !delta.is_finite() {
        return Err(invalid("the displacement is not finite"));
    }
    let mut output = polyline.clone();
    output.vertices[segment].position += delta;
    output.vertices[polyline.segment_end(segment)].position += delta;
    Ok(output)
}
pub fn vertices_between(polyline: &CurvePolyline2, from: usize, to: usize) -> Vec<usize> {
    let mut output = Vec::new();
    let count = polyline.vertices.len();
    if from >= count || to >= count || from == to {
        return output;
    }
    if !polyline.closed {
        output.extend(from.min(to) + 1..from.max(to));
        return output;
    }
    let mut index = (from + 1) % count;
    while index != to {
        output.push(index);
        index = (index + 1) % count;
    }
    output
}
fn check_range(
    polyline: &CurvePolyline2,
    mut from: usize,
    mut to: usize,
) -> Result<(usize, usize)> {
    check_vertex(polyline, from)?;
    check_vertex(polyline, to)?;
    if from == to {
        return Err(invalid(
            "the two vertices are the same vertex; pick two different ones",
        ));
    }
    if !polyline.closed && from > to {
        std::mem::swap(&mut from, &mut to);
    }
    Ok((from, to))
}
pub fn straighten(polyline: &CurvePolyline2, from: usize, to: usize) -> PolylineResult {
    let (from, to) = check_range(polyline, from, to)?;
    let between = vertices_between(polyline, from, to);
    if polyline.vertices.len() - between.len() < minimum_vertices(polyline) {
        return Err(degenerate(format!(
            "straightening would leave fewer than {} vertices",
            minimum_vertices(polyline)
        )));
    }
    if between.is_empty() && straight(polyline.vertices[from].bulge) {
        return Err(invalid(
            "the vertices are already joined by one straight segment",
        ));
    }
    let mut output = CurvePolyline2 {
        closed: polyline.closed,
        ..Default::default()
    };
    for (index, &vertex) in polyline.vertices.iter().enumerate() {
        if between.contains(&index) {
            continue;
        }
        let mut vertex = vertex;
        if index == from {
            vertex.bulge = 0.0;
        }
        output.vertices.push(vertex);
    }
    Ok(output)
}
fn walk(polyline: &CurvePolyline2, from: usize, to: usize) -> Vec<(usize, f64)> {
    let mut output = Vec::new();
    let count = polyline.vertices.len();
    let mut walked = 0.0;
    let mut index = from;
    while index != to {
        walked += polyline.segment_length(index);
        output.push(((index + 1) % count, walked));
        index = (index + 1) % count;
    }
    output
}
pub fn grade_between(polyline: &CurvePolyline2, from: usize, to: usize) -> PolylineResult {
    let (from, to) = check_range(polyline, from, to)?;
    let first = polyline.vertices[from].height;
    let last = polyline.vertices[to].height;
    let (Some(first), Some(last)) = (first, last) else {
        return Err(invalid(format!(
            "both vertices need a height to grade between them; vertex {} has none",
            if first.is_none() { from } else { to }
        )));
    };
    let steps = walk(polyline, from, to);
    let total = steps.last().map_or(0.0, |step| step.1);
    if !(total > tolerance::GEOMETRIC) {
        return Err(degenerate(
            "the two vertices are at the same place along the polyline",
        ));
    }
    let mut output = polyline.clone();
    for (index, station) in steps {
        if index != to {
            output.vertices[index].height = Some(first + (last - first) * station / total);
        }
    }
    Ok(output)
}
pub fn interpolate_heights(polyline: &CurvePolyline2) -> PolylineResult {
    let known: Vec<usize> = polyline
        .vertices
        .iter()
        .enumerate()
        .filter_map(|(index, vertex)| vertex.height.is_some().then_some(index))
        .collect();
    if known.len() < 2 {
        return Err(invalid(
            "interpolating heights needs at least two vertices with heights",
        ));
    }
    let mut output = polyline.clone();
    let mut fill = |from: usize, to: usize| {
        let steps = walk(polyline, from, to);
        let total = steps.last().map_or(0.0, |step| step.1);
        if !(total > 0.0) {
            return;
        }
        let first = polyline.vertices[from].height.unwrap();
        let last = polyline.vertices[to].height.unwrap();
        for (index, station) in steps {
            if index != to {
                output.vertices[index].height = Some(first + (last - first) * station / total);
            }
        }
    };
    for pair in known.windows(2) {
        fill(pair[0], pair[1]);
    }
    if polyline.closed {
        fill(*known.last().unwrap(), known[0]);
    }
    Ok(output)
}
fn peucker(
    polyline: &CurvePolyline2,
    order: &[usize],
    first: usize,
    last: usize,
    epsilon: f64,
    kept: &mut [bool],
) {
    if last <= first + 1 {
        return;
    }
    let start = polyline.vertices[order[first]];
    let end = polyline.vertices[order[last]];
    let chord = Segment2::new(start.position, end.position);
    let mut worst = -1.0;
    let mut worst_at = first;
    for index in first + 1..last {
        let vertex = polyline.vertices[order[index]];
        let mut deviation = chord.distance_to(vertex.position);
        if let (Some(height), Some(start_height), Some(end_height)) =
            (vertex.height, start.height, end.height)
        {
            let parameter = chord.parameter_of(vertex.position);
            deviation = max(
                deviation,
                (height - (start_height + (end_height - start_height) * parameter)).abs(),
            );
        }
        if kept[order[index]] {
            deviation = f64::INFINITY;
        }
        if deviation > worst {
            worst = deviation;
            worst_at = index;
        }
    }
    if worst > epsilon {
        kept[order[worst_at]] = true;
        peucker(polyline, order, first, worst_at, epsilon, kept);
        peucker(polyline, order, worst_at, last, epsilon, kept);
    }
}
pub fn weed(polyline: &CurvePolyline2, epsilon: f64, keep: &[bool]) -> PolylineResult {
    if !(epsilon >= 0.0) || !epsilon.is_finite() {
        return Err(invalid(
            "the weed tolerance must be a number of zero or more",
        ));
    }
    let count = polyline.vertices.len();
    if count <= minimum_vertices(polyline) {
        return Ok(polyline.clone());
    }
    let mut kept = vec![false; count];
    let copied = count.min(keep.len());
    kept[..copied].copy_from_slice(&keep[..copied]);
    for index in 0..polyline.segment_count() {
        if !straight(polyline.vertices[index].bulge) {
            kept[index] = true;
            kept[polyline.segment_end(index)] = true;
        }
    }
    let mut order = Vec::new();
    if !polyline.closed {
        kept[0] = true;
        kept[count - 1] = true;
        order.extend(0..count);
    } else {
        let start = kept.iter().position(|&keep| keep).unwrap_or(0);
        kept[start] = true;
        let mut far = start;
        let mut far_distance = -1.0;
        for index in 0..count {
            let distance = polyline.vertices[index]
                .position
                .distance_to(polyline.vertices[start].position);
            if distance > far_distance {
                far_distance = distance;
                far = index;
            }
        }
        kept[far] = true;
        order.extend((0..=count).map(|index| (start + index) % count));
    }
    let mut run_start = 0;
    for index in 1..order.len() {
        if kept[order[index]] || index + 1 == order.len() {
            peucker(polyline, &order, run_start, index, epsilon, &mut kept);
            run_start = index;
        }
    }
    if polyline.closed && kept.iter().filter(|&&keep| keep).count() < 3 {
        let mut best = 0;
        let mut best_distance = -1.0;
        let stay: Vec<usize> = kept
            .iter()
            .enumerate()
            .filter_map(|(index, &keep)| keep.then_some(index))
            .collect();
        let chord = Segment2::new(
            polyline.vertices[stay[0]].position,
            polyline.vertices[*stay.last().unwrap()].position,
        );
        for (index, &is_kept) in kept.iter().enumerate() {
            if !is_kept && chord.distance_to(polyline.vertices[index].position) > best_distance {
                best_distance = chord.distance_to(polyline.vertices[index].position);
                best = index;
            }
        }
        kept[best] = true;
    }
    Ok(CurvePolyline2 {
        closed: polyline.closed,
        vertices: polyline
            .vertices
            .iter()
            .zip(kept)
            .filter_map(|(&vertex, keep)| keep.then_some(vertex))
            .collect(),
    })
}
fn densify_parts(length: f64, step: f64) -> usize {
    if step > 0.0 {
        ((length / step - 1e-9).ceil() as usize).max(1)
    } else {
        1
    }
}
pub fn densify(polyline: &CurvePolyline2, step: f64, chord_tolerance: f64) -> PolylineResult {
    if !(step >= 0.0) || !step.is_finite() {
        return Err(invalid(
            "the densify interval must be a number of zero or more",
        ));
    }
    if !(chord_tolerance >= 0.0) || !chord_tolerance.is_finite() {
        return Err(invalid(
            "the chord tolerance must be a number of zero or more",
        ));
    }
    if step == 0.0 && chord_tolerance == 0.0 {
        return Err(invalid("give an interval, a chord tolerance, or both"));
    }
    let mut output = CurvePolyline2 {
        closed: polyline.closed,
        ..Default::default()
    };
    for index in 0..polyline.segment_count() {
        let first = polyline.vertices[index];
        let last = polyline.vertices[polyline.segment_end(index)];
        let piece = polyline.segment(index);
        let length = piece.length();
        if let CurveSegment::Arc(arc) = piece {
            if chord_tolerance > 0.0 {
                let chords = chord_arc(&arc, chord_tolerance);
                for current in 0..chords.len().saturating_sub(1) {
                    let start = if current == 0 {
                        first.position
                    } else {
                        chords[current]
                    };
                    let end = if current + 2 == chords.len() {
                        last.position
                    } else {
                        chords[current + 1]
                    };
                    let from = current as f64 / (chords.len() - 1) as f64;
                    let to = (current + 1) as f64 / (chords.len() - 1) as f64;
                    let chord_length = start.distance_to(end);
                    let parts = densify_parts(chord_length, step);
                    for part in 0..parts {
                        let fraction = part as f64 / parts as f64;
                        let mut vertex = CurveVertex {
                            position: start + (end - start) * fraction,
                            bulge: 0.0,
                            height: lerp_height(
                                first.height,
                                last.height,
                                from + (to - from) * fraction,
                            ),
                        };
                        if current == 0 && part == 0 {
                            vertex.position = first.position;
                            vertex.height = first.height;
                        }
                        output.vertices.push(vertex);
                    }
                }
                continue;
            }
            let parts = densify_parts(length, step);
            let bulge = bulge_from_sweep(arc.sweep / parts as f64);
            for part in 0..parts {
                let fraction = part as f64 / parts as f64;
                output.vertices.push(CurveVertex {
                    position: if part == 0 {
                        first.position
                    } else {
                        arc.point_at(fraction)
                    },
                    bulge,
                    height: if part == 0 {
                        first.height
                    } else {
                        lerp_height(first.height, last.height, fraction)
                    },
                });
            }
            continue;
        }
        let parts = densify_parts(length, step);
        for part in 0..parts {
            let fraction = part as f64 / parts as f64;
            output.vertices.push(CurveVertex {
                position: if part == 0 {
                    first.position
                } else {
                    first.position + (last.position - first.position) * fraction
                },
                bulge: 0.0,
                height: if part == 0 {
                    first.height
                } else {
                    lerp_height(first.height, last.height, fraction)
                },
            });
        }
    }
    if !polyline.closed
        && let Some(&last) = polyline.vertices.last()
    {
        output.vertices.push(CurveVertex { bulge: 0.0, ..last });
    }
    Ok(output)
}
pub fn merge_near_vertices(polyline: &CurvePolyline2, epsilon: f64) -> PolylineResult {
    if !(epsilon >= 0.0) || !epsilon.is_finite() {
        return Err(invalid(
            "the merge tolerance must be a number of zero or more",
        ));
    }
    let mut output = CurvePolyline2 {
        closed: polyline.closed,
        ..Default::default()
    };
    for &vertex in &polyline.vertices {
        if let Some(last) = output.vertices.last_mut()
            && last.position.distance_to(vertex.position) <= epsilon
        {
            last.bulge = vertex.bulge;
            if last.height.is_none() {
                last.height = vertex.height;
            }
            continue;
        }
        output.vertices.push(vertex);
    }
    if output.closed {
        while output.vertices.len() > 1
            && output
                .vertices
                .last()
                .unwrap()
                .position
                .distance_to(output.vertices[0].position)
                <= epsilon
        {
            output.vertices.pop();
        }
    }
    if output.vertices.len() < minimum_vertices(polyline) {
        return Err(degenerate(format!(
            "merging within {} would leave fewer than {} vertices",
            format_exact_real(epsilon),
            minimum_vertices(polyline)
        )));
    }
    Ok(output)
}
pub fn snap_to_grid(polyline: &CurvePolyline2, spacing: f64, origin: Point2) -> PolylineResult {
    if !(spacing > tolerance::GEOMETRIC) || !spacing.is_finite() {
        return Err(invalid("the grid spacing must be greater than zero"));
    }
    let mut output = polyline.clone();
    for vertex in &mut output.vertices {
        let delta = vertex.position - origin;
        vertex.position = origin
            + Vec2::new(
                (delta.x / spacing).round() * spacing,
                (delta.y / spacing).round() * spacing,
            );
    }
    merge_near_vertices(&output, tolerance::GEOMETRIC)
}
pub fn close_polyline(polyline: &CurvePolyline2) -> PolylineResult {
    if polyline.closed {
        return Err(invalid("the polyline is already closed"));
    }
    let mut output = polyline.clone();
    if output.vertices.len() > 1
        && coincide(
            output.vertices.last().unwrap().position,
            output.vertices[0].position,
        )
    {
        let height = output.vertices.last().unwrap().height;
        output.vertices.pop();
        if output.vertices[0].height.is_none() {
            output.vertices[0].height = height;
        }
    } else {
        output.vertices.last_mut().unwrap().bulge = 0.0;
    }
    output.closed = true;
    if output.vertices.len() < 3 {
        return Err(degenerate(
            "a closed polyline needs three different vertices",
        ));
    }
    Ok(output)
}
pub fn open_polyline(polyline: &CurvePolyline2) -> PolylineResult {
    if !polyline.closed {
        return Err(invalid("the polyline is already open"));
    }
    let mut output = polyline.clone();
    output.closed = false;
    output.vertices.last_mut().unwrap().bulge = 0.0;
    Ok(output)
}
pub fn change_start_vertex(polyline: &CurvePolyline2, index: usize) -> PolylineResult {
    if !polyline.closed {
        return Err(invalid(
            "only a closed polyline can start at another vertex; an open one starts at its end (use Reverse to start at the other)",
        ));
    }
    check_vertex(polyline, index)?;
    if index == 0 {
        return Err(invalid("vertex 0 is already the start"));
    }
    let mut output = polyline.clone();
    output.vertices.rotate_left(index);
    Ok(output)
}
struct Corner {
    previous: usize,
    next: usize,
    back: Vec2,
    forward: Vec2,
    back_length: f64,
    forward_length: f64,
}
fn corner_at(polyline: &CurvePolyline2, index: usize) -> Result<Corner> {
    check_vertex(polyline, index)?;
    let count = polyline.vertices.len();
    if !polyline.closed && (index == 0 || index + 1 == count) {
        return Err(invalid(format!(
            "vertex {index} is an end of the polyline; a corner is where two segments meet"
        )));
    }
    let previous = previous_vertex(polyline, index);
    let next = (index + 1) % count;
    if !straight(polyline.vertices[previous].bulge) || !straight(polyline.vertices[index].bulge) {
        return Err(invalid(format!(
            "vertex {index} is beside an arc; only a corner between two straight segments can be rounded or cut"
        )));
    }
    let vertex = polyline.vertices[index].position;
    let back = polyline.vertices[previous].position - vertex;
    let forward = polyline.vertices[next].position - vertex;
    let back_length = back.length();
    let forward_length = forward.length();
    if !(back_length > tolerance::GEOMETRIC) || !(forward_length > tolerance::GEOMETRIC) {
        return Err(degenerate(format!(
            "a segment at vertex {index} has no length"
        )));
    }
    let back = back / back_length;
    let forward = forward / forward_length;
    if back.cross(forward).abs() <= tolerance::ANGULAR {
        return Err(invalid(format!(
            "the segments at vertex {index} are in line; there is no corner to round or cut"
        )));
    }
    Ok(Corner {
        previous,
        next,
        back,
        forward,
        back_length,
        forward_length,
    })
}
fn replace_corner(
    polyline: &CurvePolyline2,
    index: usize,
    corner: &Corner,
    first: Point2,
    second: Point2,
    bulge: f64,
) -> CurvePolyline2 {
    let vertex = polyline.vertices[index];
    let previous = polyline.vertices[corner.previous];
    let next = polyline.vertices[corner.next];
    let first_fraction = 1.0 - vertex.position.distance_to(first) / corner.back_length;
    let second_fraction = vertex.position.distance_to(second) / corner.forward_length;
    let mut output = polyline.clone();
    let mut replacement = Vec::new();
    if coincide(first, previous.position) {
        output.vertices[corner.previous].bulge = bulge;
    } else {
        replacement.push(CurveVertex {
            position: first,
            bulge,
            height: lerp_height(previous.height, vertex.height, first_fraction),
        });
    }
    if !coincide(second, next.position) {
        replacement.push(CurveVertex {
            position: second,
            bulge: 0.0,
            height: lerp_height(vertex.height, next.height, second_fraction),
        });
    }
    output.vertices.splice(index..index + 1, replacement);
    output
}
pub fn check_corner(polyline: &CurvePolyline2, index: usize) -> Status {
    corner_at(polyline, index).map(|_| ())
}
pub fn fillet_vertex(polyline: &CurvePolyline2, index: usize, radius: f64) -> PolylineResult {
    if !(radius > tolerance::GEOMETRIC) || !radius.is_finite() {
        return Err(invalid("the fillet radius must be greater than zero"));
    }
    let corner = corner_at(polyline, index)?;
    let angle = corner.back.dot(corner.forward).clamp(-1.0, 1.0).acos();
    let reach = radius / (0.5 * angle).tan();
    if reach > corner.back_length + tolerance::GEOMETRIC
        || reach > corner.forward_length + tolerance::GEOMETRIC
    {
        return Err(invalid(format!(
            "a radius of {} does not fit at vertex {index}; it needs {} of each segment",
            format_exact_real(radius),
            format_exact_real(reach)
        )));
    }
    let vertex = polyline.vertices[index].position;
    let first = vertex + corner.back * min(reach, corner.back_length);
    let second = vertex + corner.forward * min(reach, corner.forward_length);
    let incoming = corner.back * -1.0;
    let turn = if incoming.cross(corner.forward) > 0.0 {
        1.0
    } else {
        -1.0
    };
    let sweep = turn * (PI - angle);
    Ok(replace_corner(
        polyline,
        index,
        &corner,
        first,
        second,
        bulge_from_sweep(sweep),
    ))
}
pub fn chamfer_vertex(
    polyline: &CurvePolyline2,
    index: usize,
    first: f64,
    second: f64,
) -> PolylineResult {
    if !(first > tolerance::GEOMETRIC)
        || !(second > tolerance::GEOMETRIC)
        || !first.is_finite()
        || !second.is_finite()
    {
        return Err(invalid("both chamfer distances must be greater than zero"));
    }
    let corner = corner_at(polyline, index)?;
    if first > corner.back_length + tolerance::GEOMETRIC
        || second > corner.forward_length + tolerance::GEOMETRIC
    {
        return Err(invalid(format!(
            "the chamfer is longer than a segment at vertex {index}"
        )));
    }
    let vertex = polyline.vertices[index].position;
    Ok(replace_corner(
        polyline,
        index,
        &corner,
        vertex + corner.back * min(first, corner.back_length),
        vertex + corner.forward * min(second, corner.forward_length),
        0.0,
    ))
}
fn height_on_segment(polyline: &CurvePolyline2, index: usize, parameter: f64) -> Option<f64> {
    let first = polyline.vertices[index].height;
    let last = polyline.vertices[polyline.segment_end(index)].height;
    if parameter <= 0.0 {
        first
    } else if parameter >= 1.0 {
        last
    } else {
        lerp_height(first, last, parameter)
    }
}
pub fn sub_path(polyline: &CurvePolyline2, from: f64, to: f64) -> CurvePolyline2 {
    let mut output = CurvePolyline2::default();
    let total = polyline.length();
    let from = from.clamp(0.0, total);
    let to = to.clamp(from, total);
    let mut walked = 0.0;
    for index in 0..polyline.segment_count() {
        let length = polyline.segment_length(index);
        let start_station = walked;
        let end_station = walked + length;
        walked = end_station;
        let first = max(from, start_station);
        let last = min(to, end_station);
        if last - first <= tolerance::GEOMETRIC || !(length > 0.0) {
            continue;
        }
        let first_fraction = if first - start_station <= tolerance::GEOMETRIC {
            0.0
        } else {
            (first - start_station) / length
        };
        let last_fraction = if end_station - last <= tolerance::GEOMETRIC {
            1.0
        } else {
            (last - start_station) / length
        };
        let piece = polyline.segment(index);
        let bulge =
            (polyline.vertices[index].bulge.atan() * (last_fraction - first_fraction)).tan();
        if output.vertices.is_empty() {
            output.vertices.push(CurveVertex {
                position: piece.point_at(first_fraction),
                bulge,
                height: height_on_segment(polyline, index, first_fraction),
            });
        } else {
            output.vertices.last_mut().unwrap().bulge = bulge;
        }
        let end = if last_fraction >= 1.0 {
            polyline.vertices[polyline.segment_end(index)].position
        } else {
            piece.point_at(last_fraction)
        };
        output.vertices.push(CurveVertex {
            position: end,
            bulge: 0.0,
            height: height_on_segment(polyline, index, last_fraction),
        });
    }
    if output.vertices.len() == 1 {
        output.vertices.clear();
    }
    output
}
pub fn wrapping_path(polyline: &CurvePolyline2, from: f64, to: f64) -> CurvePolyline2 {
    let mut head = sub_path(polyline, from, polyline.length());
    let tail = sub_path(polyline, 0.0, to);
    if head.vertices.is_empty() {
        return tail;
    }
    if tail.vertices.is_empty() {
        return head;
    }
    head.vertices.last_mut().unwrap().bulge = tail.vertices[0].bulge;
    head.vertices.extend_from_slice(&tail.vertices[1..]);
    head
}
