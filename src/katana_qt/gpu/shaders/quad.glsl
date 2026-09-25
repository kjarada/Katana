// The quad a line or a point becomes: the GLSL twin of KATANA_GPU_QUAD_VERTEX
// and kQuadBuilders in shader_library.cpp, function for function. Shared by
// both expansions, so they draw the same pixels: the geometry shaders call
// these four times per line or point, the instanced vertex shaders once per
// corner. Include after frame.glsl.
//
// `local` is in device pixels and linear in screen space (the stages declare
// it noperspective), so the fragment shaders measure its distance from the
// segment or the centre exactly. `shape` is flat.

struct QuadVertex
{
    vec4 pos;
    vec4 color;
    vec2 local;
    vec2 shape;
};

// Outside the depth range, or on its edge under OpenGL's [-w, w]: either way
// all six corners of a hidden instance are this one point, so its two
// triangles have no area and draw nothing.
QuadVertex hiddenCorner()
{
    QuadVertex o;
    o.pos = vec4(0.0, 0.0, -1.0, 1.0);
    o.color = vec4(0.0);
    o.local = vec2(0.0);
    o.shape = vec2(0.0);
    return o;
}

// MARK PULL (gpu_renderer.cpp, kMarkPull): see pulledTowardsEye in
// shader_library.cpp for the working. The clip-space blend is the same under
// every backend's correction (scene_origin.cpp, reversedZProjection): the
// correction is applied to mvp, and markPull.y is taken through that mvp.
vec4 pulledTowardsEye(vec4 c, float reach)
{
    float amount = (reach - 1.0) * markPull.x;
    if (amount <= 0.0) {
        return c; // a 1 px line at 100%: the dense TIN's edges skip the rest
    }
    vec4 p = c;
    if (eyeRel.w > 0.5) {
        float s = min(amount, 0.5);
        p = vec4(c.xy * (1.0 - s), mix(c.z, markPull.y, s), c.w * (1.0 - s));
    } else {
        p.z = c.z - amount * markPull.y;
    }
    return p.w - p.z < 0.0 ? c : p;
}

struct LineSetup
{
    vec4 ca;
    vec4 cb;
    vec4 colorA;
    vec4 colorB;
    vec2 dir;
    float len;
    float halfWidth;
    float visible;
};

LineSetup setupLine(vec4 ca, vec4 cb, vec4 colorA, vec4 colorB, float width)
{
    LineSetup s;
    s.halfWidth = max(width, 1.0) * params.z * 0.5;
    ca = pulledTowardsEye(ca, s.halfWidth + 0.5);
    cb = pulledTowardsEye(cb, s.halfWidth + 0.5);
    // Reversed Z: a point is in front of the near plane when w - z >= 0 -
    // under OpenGL's correction too (z' = 2z - w keeps z' = w at the near
    // plane). Clip the segment there before dividing.
    float da = ca.w - ca.z;
    float db = cb.w - cb.z;
    s.visible = (da < 0.0 && db < 0.0) ? 0.0 : 1.0;
    if (s.visible > 0.5 && da < 0.0) {
        float t = da / (da - db);
        ca = mix(ca, cb, t);
        colorA = mix(colorA, colorB, t);
    } else if (s.visible > 0.5 && db < 0.0) {
        float t = db / (db - da);
        cb = mix(cb, ca, t);
        colorB = mix(colorB, colorA, t);
    }
    vec2 halfSize = viewport.xy * 0.5;
    vec2 sa = ca.xy / ca.w * halfSize;
    vec2 sb = cb.xy / cb.w * halfSize;
    vec2 d = sb - sa;
    s.len = length(d);
    s.dir = s.len > 1e-6 ? d / s.len : vec2(1.0, 0.0);
    // Both ends past the same edge of the view by more than the quad reaches:
    // nothing of the line can show, so it is dropped before the rasteriser.
    vec2 reach = halfSize + s.halfWidth + 0.5;
    if (any(greaterThan(min(sa, sb), reach)) || any(lessThan(max(sa, sb), -reach))) {
        s.visible = 0.0;
    }
    s.ca = ca;
    s.cb = cb;
    s.colorA = colorA;
    s.colorB = colorB;
    return s;
}

// corner.x: 0 at end A, 1 at end B; corner.y: -1 or +1, the side. Square caps
// half a width past each end, as the software path draws them. Under Vulkan
// the screen's y runs the other way from Direct3D's (the correction flips
// clip y); the quad is symmetric about the segment, so it covers the same
// pixels.
QuadVertex lineCorner(LineSetup s, vec2 corner)
{
    vec2 halfSize = viewport.xy * 0.5;
    float extent = s.halfWidth + 0.5;
    bool atA = corner.x < 0.5;
    vec4 clip = atA ? s.ca : s.cb;
    float along = atA ? -extent : extent;
    vec2 across = vec2(-s.dir.y, s.dir.x);
    vec2 offset = across * (corner.y * extent) + s.dir * along;
    clip.xy += offset / halfSize * clip.w;
    QuadVertex o;
    o.pos = clip;
    o.color = atA ? s.colorA : s.colorB;
    o.local = vec2((atA ? 0.0 : s.len) + along, corner.y * extent);
    o.shape = vec2(s.len, s.halfWidth);
    return o;
}

// A square of side 2 * halfSize device pixels centred on `c`; corner is
// (+-1, +-1).
QuadVertex spriteCorner(vec4 c, vec4 color, float halfSize, vec2 corner)
{
    float extent = halfSize + 0.5;
    QuadVertex o;
    o.pos = c;
    o.pos.xy += corner * extent / (viewport.xy * 0.5) * c.w;
    o.color = color;
    o.local = corner * extent;
    o.shape = vec2(halfSize, 0.0);
    return o;
}

// Behind the near plane (reversed Z: depth z/w above 1), or so far past an
// edge of the view that no corner can reach back into it.
bool spriteHidden(vec4 c, float halfSize)
{
    if (c.w - c.z < 0.0) {
        return true;
    }
    vec2 halfView = viewport.xy * 0.5;
    vec2 s = abs(c.xy / c.w * halfView);
    return any(greaterThan(s, halfView + halfSize + 0.5));
}
