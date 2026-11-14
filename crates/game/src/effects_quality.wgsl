// The options' Effects Quality at Low (`effects_quality.rs`). KOTOR's
// emitter particles are drawn into a texture half the frame's width and half
// its height. Its colour holds the particles' light, already multiplied by
// how much each covers. Its alpha holds how much of the scene still shows
// through them (1 where no particle is drawn).
//
// `downsample` makes the half-size depth that particles are tested against.
// Each of its texels keeps the nearest of the four frame pixels under it, so
// a particle behind the edge of a nearer object is hidden there.
//
// `composite` lays the half-size texture over the frame. Each frame pixel
// reads the four half-size texels around it. When their depths agree with
// the pixel's own depth, the four are blended smoothly. When one of them
// lies at another depth (the pixel is at the edge of a nearer object), the
// pixel takes only the texel whose depth is closest to its own. Smoke then
// does not spill over the edge of a character standing in front of it.

#ifdef MULTISAMPLED
@group(0) @binding(0) var scene_depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(0) var scene_depth: texture_depth_2d;
#endif

#ifdef COMPOSITE
@group(0) @binding(1) var effects: texture_2d<f32>;
@group(0) @binding(2) var effects_depth: texture_depth_2d;
#endif

// When the depths of the four texels differ from the pixel's by more than
// this part of the distance, the pixel takes the closest texel alone.
const EDGE: f32 = 0.1;

fn depth_at(at: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(scene_depth));
    return textureLoad(scene_depth, clamp(at, vec2(0), size - 1), 0);
}

@fragment
fn downsample(@builtin(position) position: vec4<f32>) -> @builtin(frag_depth) f32 {
    let base = vec2<i32>(position.xy) * 2;
    // Bevy's depth is reversed: the larger value is the nearer.
    return max(
        max(depth_at(base), depth_at(base + vec2(1, 0))),
        max(depth_at(base + vec2(0, 1)), depth_at(base + vec2(1, 1))),
    );
}

#ifdef COMPOSITE
// How far apart two depths are, as a part of the farther one's distance.
// Bevy's reversed depth is the near plane over the distance, so the near
// plane cancels out.
fn gap(a: f32, b: f32) -> f32 {
    return abs(a - b) / max(max(a, b), 1e-7);
}

@fragment
fn composite(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let depth = depth_at(vec2<i32>(position.xy));
    let spot = position.xy * 0.5 - 0.5;
    let base = vec2<i32>(floor(spot));
    let f = spot - floor(spot);
    let size = vec2<i32>(textureDimensions(effects));
    var blended = vec4(0.0);
    var closest = vec4(0.0, 0.0, 0.0, 1.0);
    var closest_gap = 1e9;
    var widest_gap = 0.0;
    for (var corner = 0; corner < 4; corner++) {
        let offset = vec2(corner & 1, corner >> 1u);
        let at = clamp(base + offset, vec2(0), size - 1);
        let colour = textureLoad(effects, at, 0);
        let here = gap(depth, textureLoad(effects_depth, at, 0));
        let wx = select(1.0 - f.x, f.x, offset.x == 1);
        let wy = select(1.0 - f.y, f.y, offset.y == 1);
        blended += colour * (wx * wy);
        if here < closest_gap {
            closest = colour;
            closest_gap = here;
        }
        widest_gap = max(widest_gap, here);
    }
    return select(closest, blended, widest_gap < EDGE);
}
#endif
