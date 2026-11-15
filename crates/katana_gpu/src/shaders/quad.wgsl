// The quad a line or a point becomes: the WGSL twin of quad.glsl and of
// KATANA_GPU_QUAD_VERTEX and kQuadBuilders in shader_library.cpp, function for
// function. The C++ shared them between its two expansions; here the
// instanced vertex shaders call them once per corner, the only expansion wgpu
// has (it has no geometry stage). Joined after frame.wgsl.
//
// `local` is in device pixels and linear in screen space (noperspective in
// the GLSL, `linear` here), so the fragment shaders measure its distance from
// the segment or the centre exactly. `shape` is flat.

struct QuadVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) @interpolate(linear) local: vec2<f32>,
    @location(2) @interpolate(flat) shape: vec2<f32>,
}

// Outside the depth range: all six corners of a hidden instance are this one
// point, so its two triangles have no area and draw nothing.
fn hidden_corner() -> QuadVertex {
    var o: QuadVertex;
    o.position = vec4<f32>(0.0, 0.0, -1.0, 1.0);
    o.color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    o.local = vec2<f32>(0.0, 0.0);
    o.shape = vec2<f32>(0.0, 0.0);
    return o;
}

// MARK PULL (gpu_renderer.rs, MARK_PULL): a mark reaching `reach` device
// pixels from its centre is moved towards the eye along its own ray by
// (reach - 1) pixels' worth of world at its depth, times the pull. Along the
// ray, so it stays on the same pixels and only its depth changes. In clip
// space that is a blend towards the eye, (0, 0, mark_pull.y, 0), by the
// fraction mark_pull.x per pixel (in perspective a pixel's worth of world
// grows with the depth), or a step along the view direction, whose clip z is
// mark_pull.y (orthographic). A mark so near the eye that the pull would put
// it in front of the near plane is left where it is.
fn pulled_towards_eye(c: vec4<f32>, reach: f32) -> vec4<f32> {
    let amount = (reach - 1.0) * frame.mark_pull.x;
    if (amount <= 0.0) {
        return c; // a 1 px line at 100%: the dense TIN's edges skip the rest
    }
    var p = c;
    if (frame.eye_rel.w > 0.5) {
        let s = min(amount, 0.5);
        p = vec4<f32>(c.xy * (1.0 - s), mix(c.z, frame.mark_pull.y, s), c.w * (1.0 - s));
    } else {
        p.z = c.z - amount * frame.mark_pull.y;
    }
    if (p.w - p.z < 0.0) {
        return c;
    }
    return p;
}

struct LineSetup {
    ca: vec4<f32>,
    cb: vec4<f32>,
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    dir: vec2<f32>,
    len: f32,
    half_width: f32,
    visible: f32,
}

fn setup_line(
    clip_a: vec4<f32>,
    clip_b: vec4<f32>,
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    width: f32,
) -> LineSetup {
    var s: LineSetup;
    s.half_width = max(width, 1.0) * frame.params.z * 0.5;
    var ca = pulled_towards_eye(clip_a, s.half_width + 0.5);
    var cb = pulled_towards_eye(clip_b, s.half_width + 0.5);
    var ka = color_a;
    var kb = color_b;
    // Reversed Z: a point is in front of the near plane when its depth z/w is
    // at most 1, i.e. w - z >= 0. Clip the segment there before dividing.
    let da = ca.w - ca.z;
    let db = cb.w - cb.z;
    s.visible = 1.0;
    if (da < 0.0 && db < 0.0) {
        s.visible = 0.0;
    }
    if (s.visible > 0.5 && da < 0.0) {
        let t = da / (da - db);
        ca = mix(ca, cb, vec4<f32>(t, t, t, t));
        ka = mix(ka, kb, vec4<f32>(t, t, t, t));
    } else if (s.visible > 0.5 && db < 0.0) {
        let t = db / (db - da);
        cb = mix(cb, ca, vec4<f32>(t, t, t, t));
        kb = mix(kb, ka, vec4<f32>(t, t, t, t));
    }
    let half_size = frame.viewport.xy * 0.5;
    let sa = ca.xy / ca.w * half_size;
    let sb = cb.xy / cb.w * half_size;
    let d = sb - sa;
    s.len = length(d);
    if (s.len > 1e-6) {
        s.dir = d / s.len;
    } else {
        s.dir = vec2<f32>(1.0, 0.0);
    }
    // Both ends past the same edge of the view by more than the quad reaches:
    // nothing of the line can show, so it is dropped before the rasteriser.
    let reach = half_size + s.half_width + 0.5;
    if (any(min(sa, sb) > reach) || any(max(sa, sb) < -reach)) {
        s.visible = 0.0;
    }
    s.ca = ca;
    s.cb = cb;
    s.color_a = ka;
    s.color_b = kb;
    return s;
}

// corner.x: 0 at end A, 1 at end B; corner.y: -1 or +1, the side. Square caps
// half a width past each end, as the software path draws them. The quad is
// symmetric about the segment, so whichever way a backend's screen y runs it
// covers the same pixels.
fn line_corner(s: LineSetup, corner: vec2<f32>) -> QuadVertex {
    let half_size = frame.viewport.xy * 0.5;
    let extent = s.half_width + 0.5;
    let at_a = corner.x < 0.5;
    var clip = s.cb;
    var along = extent;
    var color = s.color_b;
    var start = s.len;
    if (at_a) {
        clip = s.ca;
        along = -extent;
        color = s.color_a;
        start = 0.0;
    }
    let across = vec2<f32>(-s.dir.y, s.dir.x);
    let offset = across * (corner.y * extent) + s.dir * along;
    clip = vec4<f32>(clip.xy + offset / half_size * clip.w, clip.z, clip.w);
    var o: QuadVertex;
    o.position = clip;
    o.color = color;
    o.local = vec2<f32>(start + along, corner.y * extent);
    o.shape = vec2<f32>(s.len, s.half_width);
    return o;
}

// A square of side 2 * half_size device pixels centred on `c`; corner is
// (+-1, +-1).
fn sprite_corner(c: vec4<f32>, color: vec4<f32>, half_size: f32, corner: vec2<f32>) -> QuadVertex {
    let extent = half_size + 0.5;
    var o: QuadVertex;
    o.position = vec4<f32>(c.xy + corner * extent / (frame.viewport.xy * 0.5) * c.w, c.z, c.w);
    o.color = color;
    o.local = corner * extent;
    o.shape = vec2<f32>(half_size, 0.0);
    return o;
}

// Behind the near plane (reversed Z: depth z/w above 1), or so far past an
// edge of the view that no corner can reach back into it.
fn sprite_hidden(c: vec4<f32>, half_size: f32) -> bool {
    if (c.w - c.z < 0.0) {
        return true;
    }
    let half_view = frame.viewport.xy * 0.5;
    let s = abs(c.xy / c.w * half_view);
    return any(s > half_view + half_size + 0.5);
}
