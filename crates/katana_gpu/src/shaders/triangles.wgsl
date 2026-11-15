// The filled triangles: triangles.vert and triangles.frag
// (kTrianglesVertex and kTrianglesFragment in shader_library.cpp). Joined
// after frame.wgsl. Colours arrive as the draw list's 0xAARRGGBB read as
// UNORM bytes in memory order - B, G, R, A - so .zyxw puts them back.

struct TriangleIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

struct TriangleOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) rel: vec3<f32>,
}

@vertex
fn vs_main(input: TriangleIn) -> TriangleOut {
    var o: TriangleOut;
    let p = exaggerate(input.position);
    o.position = frame.mvp * vec4<f32>(p, 1.0);
    o.color = input.color.zyxw;
    o.rel = p;
    return o;
}

// Per-pixel lighting from the FACE normal, taken from the screen-space
// derivatives of the interpolated position: the draw list carries no normals,
// and a TIN's facets are what a surveyor wants to see. The normal is turned to
// face the eye before lighting, so a face lit from behind gets ambient only -
// unlike the baked abs(n.l) of the software path, which lights both sides of a
// face alike (the scene builder). dpdy runs down the screen as ddy and dFdy
// do; either way the turn makes its sign irrelevant. The derivatives are
// taken first, in uniform control flow, as WGSL requires of them; the GLSL
// takes them inside the branch, whose condition is a uniform, so it is the
// same arithmetic.
@fragment
fn fs_main(input: TriangleOut) -> @location(0) vec4<f32> {
    let across = dpdx(input.rel);
    let down = dpdy(input.rel);
    var c = input.color;
    if (frame.forward_mode.w > 0.5) {
        var to_eye = -frame.forward_mode.xyz;
        if (frame.eye_rel.w > 0.5) {
            to_eye = frame.eye_rel.xyz - input.rel;
        }
        var n = cross(across, down);
        let len = length(n);
        if (len > 0.0) {
            n = n / len;
        } else {
            n = normalize(to_eye);
        }
        if (dot(n, to_eye) < 0.0) {
            n = -n;
        }
        let intensity = frame.light.w + (1.0 - frame.light.w) * clamp(dot(n, frame.light.xyz), 0.0, 1.0);
        c = vec4<f32>(c.rgb * intensity, c.a);
    }
    return c;
}
