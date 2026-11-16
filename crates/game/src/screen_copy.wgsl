// The frame copy the distortions show (`heat_haze.rs`: Bevy's view
// transmission texture, the main texture copied whole), read by where a
// point stands on the drawn frame. Loaded once, by `heat_haze::HeatHazePlugin`.
//
// The main pass draws into `view.main_pass_viewport`: the camera's viewport,
// or under DLSS its smaller render size at the same corner, the rest of the
// texture left as it was until DLSS upscales into it. So a point of the
// frame (0 to 1 across it) is a texel of that rectangle, and a tap is kept
// half a texel inside it: past it lie texels the frame never drew.

#define_import_path kjk::screen_copy

#import bevy_pbr::mesh_view_bindings::{view, view_transmission_texture, view_transmission_sampler}

/// Where the fragment at `position` (its `@builtin(position)`) stands on the
/// drawn frame, 0 to 1 across it.
fn on_frame(position: vec2<f32>) -> vec2<f32> {
    return (position - view.main_pass_viewport.xy) / view.main_pass_viewport.zw;
}

/// The frame drawn so far at `at` (0 to 1 across the frame), filtered,
/// clamped to the drawn texels.
fn frame_at(at: vec2<f32>) -> vec4<f32> {
    let corner = view.main_pass_viewport.xy;
    let size = view.main_pass_viewport.zw;
    let texel = clamp(corner + at * size, corner + 0.5, corner + size - 0.5);
    let whole = vec2<f32>(textureDimensions(view_transmission_texture));
    return textureSampleLevel(view_transmission_texture, view_transmission_sampler, texel / whole, 0.0);
}
