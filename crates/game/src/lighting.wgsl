// What the scene's surfaces (`odyssey.wgsl`) and the grass (`grass.wgsl`)
// light and fog alike. Loaded once, by `material::OdysseyMaterialPlugin`.

#define_import_path kjk::lighting

#import bevy_pbr::mesh_view_bindings::view

// The lightning's light (`weather::Flash`) at `world_position`: `colour`
// falling off to nothing at the radius `at.w` round `at.xyz`; none while
// that radius is 0.
fn flash(world_position: vec3<f32>, at: vec4<f32>, colour: vec3<f32>) -> vec3<f32> {
    let radius = at.w;
    let falloff = max(0.0, 1.0 - distance(world_position, at.xyz) / max(radius, 1e-3));
    return colour * falloff * step(1e-3, radius);
}

// The area's linear fog (the ARE's `SunFogNear` and `SunFogFar`,
// `docs/GRASS-WEATHER.md`), by the straight-line distance from the eye:
// none nearer than `range.x`, all of `fog_colour` from `range.y` on, in
// proportion between. A far edge at or before the near one turns the fog
// on at once at the far edge.
fn fog(colour: vec3<f32>, world_position: vec3<f32>, range: vec4<f32>, fog_colour: vec3<f32>) -> vec3<f32> {
    let distance = length(world_position - view.world_position);
    let near = range.x;
    let far = range.y;
    let amount = 1.0 - clamp((far - distance) / max(far - near, 1e-4), 0.0, 1.0);
    return mix(colour, fog_colour, amount);
}
