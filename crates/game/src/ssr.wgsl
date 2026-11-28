// Screen-Space Reflections pass (`ssr.rs`), running in linear HDR after
// `main_opaque_pass_3d` and restoring `ViewTarget.a = 1.0`.
//
// Opaque smooth/metallic/env-mapped surfaces encode `(r_bin << 4 | s_bin) + 2.0`
// into `ViewTarget.a` in `odyssey.wgsl`, so matte stone, sand, and skin take zero
// SSR cost and get zero unwanted shine.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import bevy_render::view::View

struct SsrParams {
    // [vp_min_x, vp_min_y, vp_max_x, vp_max_y] in pixels.
    bounds: vec4<u32>,
    // x: frame_count, y: debug_mode (1 = isolated SSR contribution), zw: unused.
    frame_debug: vec4<u32>,
}

@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var normal_tex: texture_2d<f32>;
@group(0) @binding(3) var linear_sampler: sampler;
@group(0) @binding(4) var<uniform> view: View;
@group(0) @binding(5) var<uniform> ssr: SsrParams;

fn view_distance(depth: f32) -> f32 {
    let projection = view.clip_from_view;
    if projection[3][3] < 0.5 {
        return projection[3][2] / max(depth, 1e-9);
    }
    return (projection[3][2] - depth) / projection[2][2];
}

fn ign_noise(pixel_xy: vec2<f32>, phase: f32) -> f32 {
    let shifted = pixel_xy + vec2<f32>(phase * 5.588238, phase * 5.588238);
    return fract(52.9829189 * fract(dot(shifted, vec2<f32>(0.06711056, 0.00583715))));
}

fn world_from_screen(frame_uv: vec2<f32>, depth_val: f32) -> vec3<f32> {
    let ndc = vec4<f32>(frame_uv.x * 2.0 - 1.0, 1.0 - frame_uv.y * 2.0, depth_val, 1.0);
    let world_h = view.world_from_clip * ndc;
    let w_sign = select(-1.0, 1.0, world_h.w >= 0.0);
    return world_h.xyz / (max(abs(world_h.w), 1e-6) * w_sign);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let pixel_u = vec2<u32>(in.position.xy);
    let tex_size_i = vec2<i32>(textureDimensions(scene_tex));
    let pixel_i = clamp(vec2<i32>(pixel_u), vec2<i32>(0), tex_size_i - vec2<i32>(1));
    let src = textureLoad(scene_tex, pixel_i, 0i);
    if any(pixel_u < ssr.bounds.xy) || any(pixel_u >= ssr.bounds.zw) {
        return vec4<f32>(src.rgb, 1.0);
    }
    if src.a < 2.5 {
        if ssr.frame_debug.y != 0u {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0);
        }
        return vec4<f32>(src.rgb, 1.0);
    }
    let depth_size_i = vec2<i32>(textureDimensions(depth_tex));
    let depth_i = clamp(vec2<i32>(pixel_u), vec2<i32>(0), depth_size_i - vec2<i32>(1));
    let depth_val = textureLoad(depth_tex, depth_i, 0i);
    if depth_val <= 1e-6 {
        return vec4<f32>(src.rgb, 1.0);
    }
    let packed = u32(max(round(src.a - 2.0), 0.0));
    let r_bin = min(packed >> 4u, 15u);
    let s_bin = min(packed & 15u, 15u);
    let roughness = (f32(r_bin) / 15.0) * 0.52;
    let refl_str = f32(s_bin) / 15.0;

    let vp_origin = vec2<f32>(ssr.bounds.xy);
    let vp_size = max(vec2<f32>(ssr.bounds.zw - ssr.bounds.xy), vec2<f32>(1.0));
    let pixel_f = in.position.xy;
    let frame_uv = (pixel_f - vp_origin) / vp_size;
    let world_pos = world_from_screen(frame_uv, depth_val);

    let norm_size_i = vec2<i32>(textureDimensions(normal_tex));
    let norm_i = clamp(vec2<i32>(pixel_u), vec2<i32>(0), norm_size_i - vec2<i32>(1));
    let raw_n = textureLoad(normal_tex, norm_i, 0i).rgb * 2.0 - vec3<f32>(1.0);
    if dot(raw_n, raw_n) <= 1e-6 {
        return vec4<f32>(src.rgb, 1.0);
    }
    let surf_n = normalize(raw_n);
    let view_vec = view.world_position - world_pos;
    let cam_dist = length(view_vec);
    let view_dir = view_vec / max(cam_dist, 1e-4);
    let refl_dir = reflect(-view_dir, surf_n);

    // Reject reflections pointing predominantly back into the camera off grazing walls cleanly,
    // while keeping grazing floor/wall reflections sharp.
    let origin = world_pos + surf_n * (0.025 + cam_dist * 0.002);
    let jitter = ign_noise(pixel_f, f32(ssr.frame_debug.x & 63u));
    var cur_step = 0.10;
    var ray_t = cur_step * (0.35 + 0.65 * jitter);
    var prev_t = 0.0;
    var found = false;
    var hit_uv = vec2<f32>(0.0);
    var hit_t = 0.0;
    let max_dist = 18.0;

    for (var i = 0u; i < 20u; i++) {
        if ray_t > max_dist {
            break;
        }
        let march_pos = origin + refl_dir * ray_t;
        let clip_pos = view.clip_from_world * vec4<f32>(march_pos, 1.0);
        if clip_pos.w <= 1e-4 {
            break;
        }
        let ndc_xy = clip_pos.xy / clip_pos.w;
        if abs(ndc_xy.x) > 1.0 || abs(ndc_xy.y) > 1.0 {
            break;
        }
        let uv = vec2<f32>(ndc_xy.x * 0.5 + 0.5, 0.5 - ndc_xy.y * 0.5);
        let tap = clamp(vec2<i32>(vp_origin + uv * vp_size), vec2<i32>(ssr.bounds.xy), vec2<i32>(ssr.bounds.zw) - vec2<i32>(1));
        let tap_depth = textureLoad(depth_tex, tap, 0i);
        if tap_depth > 1e-6 {
            let ray_dist = view_distance(clip_pos.z / clip_pos.w);
            let occ_dist = view_distance(tap_depth);
            let delta = ray_dist - occ_dist;
            let thickness = clamp(0.18 + cur_step * 0.85, 0.18, 1.35);
            if delta > 0.015 && delta < thickness {
                found = true;
                hit_uv = uv;
                hit_t = ray_t;
                break;
            }
        }
        prev_t = ray_t;
        cur_step = cur_step * 1.18;
        ray_t = ray_t + cur_step;
    }

    if !found {
        if ssr.frame_debug.y != 0u {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0);
        }
        return vec4<f32>(src.rgb, 1.0);
    }

    // 4 binary refinement steps for sub-step contact accuracy.
    var lo_t = prev_t;
    var hi_t = hit_t;
    for (var j = 0u; j < 4u; j++) {
        let mid_t = 0.5 * (lo_t + hi_t);
        let mid_pos = origin + refl_dir * mid_t;
        let clip_pos = view.clip_from_world * vec4<f32>(mid_pos, 1.0);
        if clip_pos.w <= 1e-4 {
            break;
        }
        let ndc_xy = clip_pos.xy / clip_pos.w;
        if abs(ndc_xy.x) > 1.0 || abs(ndc_xy.y) > 1.0 {
            break;
        }
        let uv = vec2<f32>(ndc_xy.x * 0.5 + 0.5, 0.5 - ndc_xy.y * 0.5);
        let tap = clamp(vec2<i32>(vp_origin + uv * vp_size), vec2<i32>(ssr.bounds.xy), vec2<i32>(ssr.bounds.zw) - vec2<i32>(1));
        let tap_depth = textureLoad(depth_tex, tap, 0i);
        let ray_dist = view_distance(clip_pos.z / clip_pos.w);
        let occ_dist = view_distance(tap_depth);
        let delta = ray_dist - occ_dist;
        if tap_depth > 1e-6 && delta > 0.01 && delta < 1.35 {
            hi_t = mid_t;
            hit_uv = uv;
        } else {
            lo_t = mid_t;
        }
    }

    // Read scene radiance at hit point (clamped inside main_pass_viewport).
    let whole_size = max(vec2<f32>(tex_size_i), vec2<f32>(1.0));
    let sample_texel = clamp(vp_origin + hit_uv * vp_size, vp_origin + 0.5, vp_origin + vp_size - 0.5);
    let hit_col = min(
        textureSampleLevel(scene_tex, linear_sampler, sample_texel / whole_size, 0.0).rgb,
        vec3<f32>(6.0),
    );

    // Reject hits whose surface faces away from our incoming reflection vector slightly.
    let hit_i = clamp(vec2<i32>(sample_texel), vec2<i32>(0), norm_size_i - vec2<i32>(1));
    let hit_raw_n = textureLoad(normal_tex, hit_i, 0i).rgb * 2.0 - vec3<f32>(1.0);
    var back_fade = 1.0;
    if dot(hit_raw_n, hit_raw_n) > 1e-6 {
        let hit_n = normalize(hit_raw_n);
        back_fade = smoothstep(-0.25, 0.25, -dot(hit_n, refl_dir));
    }

    let edge_u = smoothstep(0.0, 0.10, min(hit_uv.x, 1.0 - hit_uv.x));
    let edge_v = smoothstep(0.0, 0.12, min(hit_uv.y, 1.0 - hit_uv.y));
    let dist_fade = 1.0 - smoothstep(max_dist * 0.55, max_dist, hi_t);
    let rough_fade = 1.0 - smoothstep(0.06, 0.52, roughness);
    let ndotv = max(dot(surf_n, view_dir), 0.0);
    let f0 = mix(0.04, 0.72, refl_str);
    let fresnel = f0 + (1.0 - f0) * pow(1.0 - ndotv, 5.0);
    let weight = clamp(fresnel * refl_str * rough_fade * edge_u * edge_v * dist_fade * back_fade, 0.0, 0.78);

    if ssr.frame_debug.y != 0u {
        return vec4<f32>(hit_col * weight, 1.0);
    }
    let out_col = src.rgb * (1.0 - weight * 0.42) + hit_col * weight;
    return vec4<f32>(out_col, 1.0);
}
