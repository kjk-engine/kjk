// Ambient occlusion over the frame the scene camera draws
// (`ambient_occlusion`). Three compute passes, one after another, each built
// from this file with its own shader def.
//
// These are Bevy 0.19.1's own passes (`bevy_pbr/src/ssao/`, MIT or Apache
// 2.0): `preprocess_depth.wgsl` as DEPTH_PASS, `ssao.wgsl` as
// OCCLUSION_PASS and `spatial_denoise.wgsl` as DENOISE_PASS. The occlusion
// is Bevy's visibility bitmask method, which is based on Intel's XeGTAO.
//
// Two things are changed. The first is the size of the frame. Bevy measures
// a place on the frame by the view's size (`view.viewport`), which is the
// window's. Under an upscaler the camera draws a smaller frame
// (`view.main_pass_viewport`), in the top left corner of the window-sized
// depth and normal textures. So Bevy's passes worked out each pixel's place
// in the room as if the frame filled the window, and the occlusion came out
// wrong. They also ran over the whole window. Here a place on the frame is
// measured on the drawn frame's size, and a texture is read by its own size
// (`textureDimensions`). At the window's size the three sizes are the same.
// The passes also run over the drawn frame only. The second change is how a
// span of the horizon is counted (`updateSectors`).

#import bevy_render::view::View
#import bevy_render::globals::Globals
#import bevy_render::maths::{PI, HALF_PI, fast_acos}

@group(1) @binding(0) var point_clamp_sampler: sampler;
@group(1) @binding(1) var linear_clamp_sampler: sampler;
@group(1) @binding(2) var<uniform> view: View;

// The size of the frame drawn this frame, smaller than the window's under
// an upscaler.
fn drawn() -> vec2<f32> {
    return view.main_pass_viewport.zw;
}

// Whether a pixel lies outside the frame drawn this frame.
fn outside(pixel: vec2<i32>) -> bool {
    return any(vec2<f32>(pixel) >= drawn());
}

#ifdef DEPTH_PASS
// The depth as 5 levels of detail. Each level holds a weighted average of
// 4 depths of the level before.

@group(0) @binding(0) var input_depth: texture_depth_2d;
#ifdef USE_R16FLOAT
@group(0) @binding(1) var preprocessed_depth_mip0: texture_storage_2d<r16float, write>;
@group(0) @binding(2) var preprocessed_depth_mip1: texture_storage_2d<r16float, write>;
@group(0) @binding(3) var preprocessed_depth_mip2: texture_storage_2d<r16float, write>;
@group(0) @binding(4) var preprocessed_depth_mip3: texture_storage_2d<r16float, write>;
@group(0) @binding(5) var preprocessed_depth_mip4: texture_storage_2d<r16float, write>;
#else
@group(0) @binding(1) var preprocessed_depth_mip0: texture_storage_2d<r32float, write>;
@group(0) @binding(2) var preprocessed_depth_mip1: texture_storage_2d<r32float, write>;
@group(0) @binding(3) var preprocessed_depth_mip2: texture_storage_2d<r32float, write>;
@group(0) @binding(4) var preprocessed_depth_mip3: texture_storage_2d<r32float, write>;
@group(0) @binding(5) var preprocessed_depth_mip4: texture_storage_2d<r32float, write>;
#endif

fn weighted_average(depth0: f32, depth1: f32, depth2: f32, depth3: f32) -> f32 {
    let depth_range_scale_factor = 0.75;
    let effect_radius = depth_range_scale_factor * 0.5 * 1.457;
    let falloff_range = 0.615 * effect_radius;
    let falloff_from = effect_radius * (1.0 - 0.615);
    let falloff_mul = -1.0 / falloff_range;
    let falloff_add = falloff_from / falloff_range + 1.0;

    let min_depth = min(min(depth0, depth1), min(depth2, depth3));
    let weight0 = saturate((depth0 - min_depth) * falloff_mul + falloff_add);
    let weight1 = saturate((depth1 - min_depth) * falloff_mul + falloff_add);
    let weight2 = saturate((depth2 - min_depth) * falloff_mul + falloff_add);
    let weight3 = saturate((depth3 - min_depth) * falloff_mul + falloff_add);
    let weight_total = weight0 + weight1 + weight2 + weight3;

    return ((weight0 * depth0) + (weight1 * depth1) + (weight2 * depth2) + (weight3 * depth3)) / weight_total;
}

var<workgroup> previous_mip_depth: array<array<f32, 8>, 8>;

@compute
@workgroup_size(8, 8, 1)
fn preprocess_depth(@builtin(global_invocation_id) global_id: vec3<u32>, @builtin(local_invocation_id) local_id: vec3<u32>) {
    let base_coordinates = vec2<i32>(global_id.xy);

    let pixel_coordinates0 = base_coordinates * 2i;
    let pixel_coordinates1 = pixel_coordinates0 + vec2<i32>(1i, 0i);
    let pixel_coordinates2 = pixel_coordinates0 + vec2<i32>(0i, 1i);
    let pixel_coordinates3 = pixel_coordinates0 + vec2<i32>(1i, 1i);
    // Changed from Bevy: the depth texture's own size.
    let depths_uv = vec2<f32>(pixel_coordinates0) / vec2<f32>(textureDimensions(input_depth));
    let depths = textureGather(input_depth, point_clamp_sampler, depths_uv, vec2<i32>(1i, 1i));
    textureStore(preprocessed_depth_mip0, pixel_coordinates0, vec4<f32>(depths.w, 0.0, 0.0, 0.0));
    textureStore(preprocessed_depth_mip0, pixel_coordinates1, vec4<f32>(depths.z, 0.0, 0.0, 0.0));
    textureStore(preprocessed_depth_mip0, pixel_coordinates2, vec4<f32>(depths.x, 0.0, 0.0, 0.0));
    textureStore(preprocessed_depth_mip0, pixel_coordinates3, vec4<f32>(depths.y, 0.0, 0.0, 0.0));

    let depth_mip1 = weighted_average(depths.w, depths.z, depths.x, depths.y);
    textureStore(preprocessed_depth_mip1, base_coordinates, vec4<f32>(depth_mip1, 0.0, 0.0, 0.0));
    previous_mip_depth[local_id.x][local_id.y] = depth_mip1;

    workgroupBarrier();

    if all(local_id.xy % vec2<u32>(2u) == vec2<u32>(0u)) {
        let depth0 = previous_mip_depth[local_id.x + 0u][local_id.y + 0u];
        let depth1 = previous_mip_depth[local_id.x + 1u][local_id.y + 0u];
        let depth2 = previous_mip_depth[local_id.x + 0u][local_id.y + 1u];
        let depth3 = previous_mip_depth[local_id.x + 1u][local_id.y + 1u];
        let depth_mip2 = weighted_average(depth0, depth1, depth2, depth3);
        textureStore(preprocessed_depth_mip2, base_coordinates / 2i, vec4<f32>(depth_mip2, 0.0, 0.0, 0.0));
        previous_mip_depth[local_id.x][local_id.y] = depth_mip2;
    }

    workgroupBarrier();

    if all(local_id.xy % vec2<u32>(4u) == vec2<u32>(0u)) {
        let depth0 = previous_mip_depth[local_id.x + 0u][local_id.y + 0u];
        let depth1 = previous_mip_depth[local_id.x + 2u][local_id.y + 0u];
        let depth2 = previous_mip_depth[local_id.x + 0u][local_id.y + 2u];
        let depth3 = previous_mip_depth[local_id.x + 2u][local_id.y + 2u];
        let depth_mip3 = weighted_average(depth0, depth1, depth2, depth3);
        textureStore(preprocessed_depth_mip3, base_coordinates / 4i, vec4<f32>(depth_mip3, 0.0, 0.0, 0.0));
        previous_mip_depth[local_id.x][local_id.y] = depth_mip3;
    }

    workgroupBarrier();

    if all(local_id.xy % vec2<u32>(8u) == vec2<u32>(0u)) {
        let depth0 = previous_mip_depth[local_id.x + 0u][local_id.y + 0u];
        let depth1 = previous_mip_depth[local_id.x + 4u][local_id.y + 0u];
        let depth2 = previous_mip_depth[local_id.x + 0u][local_id.y + 4u];
        let depth3 = previous_mip_depth[local_id.x + 4u][local_id.y + 4u];
        let depth_mip4 = weighted_average(depth0, depth1, depth2, depth3);
        textureStore(preprocessed_depth_mip4, base_coordinates / 8i, vec4<f32>(depth_mip4, 0.0, 0.0, 0.0));
    }
}
#endif

#ifdef OCCLUSION_PASS
// The occlusion of each pixel, from the depth's levels and the normals.

@group(0) @binding(0) var preprocessed_depth: texture_2d<f32>;
@group(0) @binding(1) var normals: texture_2d<f32>;
@group(0) @binding(2) var hilbert_index_lut: texture_2d<u32>;
#ifdef USE_R16FLOAT
@group(0) @binding(3) var ambient_occlusion: texture_storage_2d<r16float, write>;
#else
@group(0) @binding(3) var ambient_occlusion: texture_storage_2d<r32float, write>;
#endif
@group(0) @binding(4) var depth_differences: texture_storage_2d<r32uint, write>;
@group(0) @binding(5) var<uniform> globals: Globals;
@group(0) @binding(6) var<uniform> thickness: f32;

fn load_noise(pixel_coordinates: vec2<i32>) -> vec2<f32> {
    var index = textureLoad(hilbert_index_lut, pixel_coordinates % 64, 0).r;

#ifdef TEMPORAL_JITTER
    index += 288u * (globals.frame_count % 64u);
#endif

    return fract(0.5 + f32(index) * vec2<f32>(0.75487766624669276005, 0.5698402909980532659114));
}

// Changed from Bevy: a place on the drawn frame (0 to 1 over its size) as
// a place in a texture of the window's size. It stays half a
// texel inside the drawn frame, so a filtered read takes nothing from
// outside it.
fn in_texture(uv: vec2<f32>, texels: vec2<f32>) -> vec2<f32> {
    let size = drawn();
    let kept = clamp(uv, 0.5 / size, 1.0 - 0.5 / size);
    return kept * size / texels;
}

fn calculate_neighboring_depth_differences(pixel_coordinates: vec2<i32>) -> f32 {
    // Changed from Bevy: the texture's own size.
    let uv = vec2<f32>(pixel_coordinates) / vec2<f32>(textureDimensions(preprocessed_depth, 0));
    let depths_upper_left = textureGather(0, preprocessed_depth, point_clamp_sampler, uv);
    let depths_bottom_right = textureGather(0, preprocessed_depth, point_clamp_sampler, uv, vec2<i32>(1i, 1i));
    let depth_center = depths_upper_left.y;
    let depth_left = depths_upper_left.x;
    let depth_top = depths_upper_left.z;
    let depth_bottom = depths_bottom_right.x;
    let depth_right = depths_bottom_right.z;

    var edge_info = vec4<f32>(depth_left, depth_right, depth_top, depth_bottom) - depth_center;
    let slope_left_right = (edge_info.y - edge_info.x) * 0.5;
    let slope_top_bottom = (edge_info.w - edge_info.z) * 0.5;
    let edge_info_slope_adjusted = edge_info + vec4<f32>(slope_left_right, -slope_left_right, slope_top_bottom, -slope_top_bottom);
    edge_info = min(abs(edge_info), abs(edge_info_slope_adjusted));
    let bias = 0.25;
    let scale = depth_center * 0.011;
    edge_info = saturate((1.0 + bias) - edge_info / scale);

    let edge_info_packed = vec4<u32>(pack4x8unorm(edge_info), 0u, 0u, 0u);
    textureStore(depth_differences, pixel_coordinates, edge_info_packed);

    return depth_center;
}

fn load_normal_view_space(uv: vec2<f32>) -> vec3<f32> {
    let at = in_texture(uv, vec2<f32>(textureDimensions(normals, 0)));
    var world_normal = textureSampleLevel(normals, point_clamp_sampler, at, 0.0).xyz;
    world_normal = (world_normal * 2.0) - 1.0;
    let view_from_world = mat3x3<f32>(
        view.view_from_world[0].xyz,
        view.view_from_world[1].xyz,
        view.view_from_world[2].xyz,
    );
    return view_from_world * world_normal;
}

fn reconstruct_view_space_position(depth: f32, uv: vec2<f32>) -> vec3<f32> {
    let clip_xy = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - 2.0 * uv.y);
    let t = view.view_from_clip * vec4<f32>(clip_xy, depth, 1.0);
    let view_xyz = t.xyz / t.w;
    return view_xyz;
}

fn load_and_reconstruct_view_space_position(uv: vec2<f32>, sample_mip_level: f32) -> vec3<f32> {
    let at = in_texture(uv, vec2<f32>(textureDimensions(preprocessed_depth, 0)));
    let depth = textureSampleLevel(preprocessed_depth, linear_clamp_sampler, at, sample_mip_level).r;
    return reconstruct_view_space_position(depth, uv);
}

// The share of a sector a span may take and still hide none of it.
const SLIVER: f32 = 0.3;

fn updateSectors(
    min_horizon: f32,
    max_horizon: f32,
    samples_per_slice: f32,
    bitmask: u32,
) -> u32 {
    let start_horizon = u32(min_horizon * samples_per_slice);
    // Changed from Bevy, which rounds the span up to whole sectors. A point
    // in the surface's own plane spans a sliver of a sector. Rounding that
    // up hid a whole sector of each slice, so a flat floor in the open kept
    // only 0.83 of its light. Here a span hides a sector only past
    // `SLIVER` of one. Such a floor then keeps about 0.93. Rounding to the
    // nearest sector kept about 0.95, but also lost much of the darkening
    // in corners.
    let angle_horizon = u32(max(0.0, ceil((max_horizon - min_horizon) * samples_per_slice - SLIVER)));

    return insertBits(bitmask, 0xFFFFFFFFu, start_horizon, angle_horizon);
}

fn processSample(
    delta_position: vec3<f32>,
    view_vec: vec3<f32>,
    sampling_direction: f32,
    n: vec2<f32>,
    samples_per_slice: f32,
    bitmask: ptr<function, u32>,
) {
    let delta_position_back_face = delta_position - view_vec * thickness;

    var front_back_horizon = vec2(
        fast_acos(dot(normalize(delta_position), view_vec)),
        fast_acos(dot(normalize(delta_position_back_face), view_vec)),
    );

    front_back_horizon = saturate(fma(vec2(sampling_direction), -front_back_horizon * (1.0 / PI), n));
    front_back_horizon = select(front_back_horizon.xy, front_back_horizon.yx, sampling_direction >= 0.0);

    *bitmask = updateSectors(front_back_horizon.x, front_back_horizon.y, samples_per_slice, *bitmask);
}

@compute
@workgroup_size(8, 8, 1)
fn ssao(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let pixel_coordinates = vec2<i32>(global_id.xy);
    if outside(pixel_coordinates) {
        return;
    }
    let slice_count = f32(#SLICE_COUNT);
    let samples_per_slice_side = f32(#SAMPLES_PER_SLICE_SIDE);
    let effect_radius = 0.5 * 1.457;

    // Changed from Bevy: the drawn frame's size.
    let uv = (vec2<f32>(pixel_coordinates) + 0.5) / drawn();

    var pixel_depth = calculate_neighboring_depth_differences(pixel_coordinates);
    pixel_depth += 0.00001;

    let pixel_position = reconstruct_view_space_position(pixel_depth, uv);
    let pixel_normal = load_normal_view_space(uv);
    let view_vec = normalize(-pixel_position);

    let noise = load_noise(pixel_coordinates);
    let sample_scale = (-0.5 * effect_radius * view.clip_from_view[0][0]) / pixel_position.z;

    var visibility = 0.0;
    var occluded_sample_count = 0u;
    for (var slice_t = 0.0; slice_t < slice_count; slice_t += 1.0) {
        let slice = slice_t + noise.x;
        let phi = (PI / slice_count) * slice;
        let omega = vec2<f32>(cos(phi), sin(phi));

        let direction = vec3<f32>(omega.xy, 0.0);
        let orthographic_direction = direction - (dot(direction, view_vec) * view_vec);
        let axis = cross(direction, view_vec);
        let projected_normal = pixel_normal - axis * dot(pixel_normal, axis);
        let projected_normal_length = length(projected_normal);

        let sign_norm = sign(dot(orthographic_direction, projected_normal));
        let cos_norm = saturate(dot(projected_normal, view_vec) / projected_normal_length);
        let n = vec2((HALF_PI - sign_norm * fast_acos(cos_norm)) * (1.0 / PI));

        var bitmask = 0u;

        let sample_mul = vec2<f32>(omega.x, -omega.y) * sample_scale;
        for (var sample_t = 0.0; sample_t < samples_per_slice_side; sample_t += 1.0) {
            var sample_noise = (slice_t + sample_t * samples_per_slice_side) * 0.6180339887498948482;
            sample_noise = fract(noise.y + sample_noise);

            var s = (sample_t + sample_noise) / samples_per_slice_side;
            s *= s;
            let sample = s * sample_mul;

            let sample_mip_level = clamp(log2(length(sample * drawn())) - 3.3, 0.0, 5.0);
            let sample_position_1 = load_and_reconstruct_view_space_position(uv + sample, sample_mip_level);
            let sample_position_2 = load_and_reconstruct_view_space_position(uv - sample, sample_mip_level);

            let sample_difference_1 = sample_position_1 - pixel_position;
            let sample_difference_2 = sample_position_2 - pixel_position;

            processSample(sample_difference_1, view_vec, -1.0, n, samples_per_slice_side * 2.0, &bitmask);
            processSample(sample_difference_2, view_vec, 1.0, n, samples_per_slice_side * 2.0, &bitmask);
        }

        occluded_sample_count += countOneBits(bitmask);
    }

    visibility = 1.0 - f32(occluded_sample_count) / (slice_count * 2.0 * samples_per_slice_side);

    visibility = clamp(visibility, 0.03, 1.0);

    textureStore(ambient_occlusion, pixel_coordinates, vec4<f32>(visibility, 0.0, 0.0, 0.0));
}
#endif

#ifdef DENOISE_PASS
// A 3 by 3 blur of the occlusion that keeps to edges in the depth.

@group(0) @binding(0) var ambient_occlusion_noisy: texture_2d<f32>;
@group(0) @binding(1) var depth_differences: texture_2d<u32>;
#ifdef USE_R16FLOAT
@group(0) @binding(2) var ambient_occlusion: texture_storage_2d<r16float, write>;
#else
@group(0) @binding(2) var ambient_occlusion: texture_storage_2d<r32float, write>;
#endif

@compute
@workgroup_size(8, 8, 1)
fn spatial_denoise(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let pixel_coordinates = vec2<i32>(global_id.xy);
    if outside(pixel_coordinates) {
        return;
    }
    // Changed from Bevy: the texture's own size.
    let uv = vec2<f32>(pixel_coordinates) / vec2<f32>(textureDimensions(ambient_occlusion_noisy, 0));

    let edges0 = textureGather(0, depth_differences, point_clamp_sampler, uv);
    let edges1 = textureGather(0, depth_differences, point_clamp_sampler, uv, vec2<i32>(2i, 0i));
    let edges2 = textureGather(0, depth_differences, point_clamp_sampler, uv, vec2<i32>(1i, 2i));
    let visibility0 = textureGather(0, ambient_occlusion_noisy, point_clamp_sampler, uv);
    let visibility1 = textureGather(0, ambient_occlusion_noisy, point_clamp_sampler, uv, vec2<i32>(2i, 0i));
    let visibility2 = textureGather(0, ambient_occlusion_noisy, point_clamp_sampler, uv, vec2<i32>(0i, 2i));
    let visibility3 = textureGather(0, ambient_occlusion_noisy, point_clamp_sampler, uv, vec2<i32>(2i, 2i));

    let left_edges = unpack4x8unorm(edges0.x);
    let right_edges = unpack4x8unorm(edges1.x);
    let top_edges = unpack4x8unorm(edges0.z);
    let bottom_edges = unpack4x8unorm(edges2.w);
    var center_edges = unpack4x8unorm(edges0.y);
    center_edges *= vec4<f32>(left_edges.y, right_edges.x, top_edges.w, bottom_edges.z);

    let center_weight = 1.2;
    let left_weight = center_edges.x;
    let right_weight = center_edges.y;
    let top_weight = center_edges.z;
    let bottom_weight = center_edges.w;
    let top_left_weight = 0.425 * (top_weight * top_edges.x + left_weight * left_edges.z);
    let top_right_weight = 0.425 * (top_weight * top_edges.y + right_weight * right_edges.z);
    let bottom_left_weight = 0.425 * (bottom_weight * bottom_edges.x + left_weight * left_edges.w);
    let bottom_right_weight = 0.425 * (bottom_weight * bottom_edges.y + right_weight * right_edges.w);

    let center_visibility = visibility0.y;
    let left_visibility = visibility0.x;
    let right_visibility = visibility1.x;
    let top_visibility = visibility0.z;
    let bottom_visibility = visibility2.z;
    let top_left_visibility = visibility0.w;
    let top_right_visibility = visibility1.w;
    let bottom_left_visibility = visibility2.w;
    let bottom_right_visibility = visibility3.w;

    var sum = center_visibility * center_weight;
    sum += left_visibility * left_weight;
    sum += right_visibility * right_weight;
    sum += top_visibility * top_weight;
    sum += bottom_visibility * bottom_weight;
    sum += top_left_visibility * top_left_weight;
    sum += top_right_visibility * top_right_weight;
    sum += bottom_left_visibility * bottom_left_weight;
    sum += bottom_right_visibility * bottom_right_weight;

    var sum_weight = center_weight;
    sum_weight += left_weight;
    sum_weight += right_weight;
    sum_weight += top_weight;
    sum_weight += bottom_weight;
    sum_weight += top_left_weight;
    sum_weight += top_right_weight;
    sum_weight += bottom_left_weight;
    sum_weight += bottom_right_weight;

    let denoised_visibility = sum / sum_weight;

    textureStore(ambient_occlusion, pixel_coordinates, vec4<f32>(denoised_visibility, 0.0, 0.0, 0.0));
}
#endif
