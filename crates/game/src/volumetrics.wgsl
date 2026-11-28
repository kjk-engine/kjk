// Volumetric Fog / Light Shafts & Local Fitted/Saber Light Cones (`volumetrics.rs`).
//
// Runs in linear HDR after `Core3dSystems::MainPass` and before temporal resolve /
// upscaling (`Core3dSystems::EarlyPostProcess`) so TAA / Upscaler / DLSS stabilize
// the 16-step temporally jittered raymarch at render resolution.
//
// Designed around KJK's moody lighting direction: adds luminous scattering only
// inside unoccluded sunbeams and near bright light fittings/sabers, never flat haze
// over shadowed indoor blacks.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import bevy_render::view::View

const VOL_LIGHTS: u32 = 16u;
const PI: f32 = 3.14159265;

struct VolumetricLight {
    // xyz: world position, w: radius
    pos_radius: vec4<f32>,
    // rgb: linear color * intensity
    color_strength: vec4<f32>,
    // xyz: normalized aim direction, w: cone threshold (0 = omnidirectional)
    aim_cone: vec4<f32>,
}

struct VolumetricUniform {
    // [vp_min_x, vp_min_y, vp_max_x, vp_max_y] in pixels.
    bounds: vec4<u32>,
    // xyz: dir toward sun 1, w: 1.0 if active else 0.0.
    sun_dir: vec4<f32>,
    // rgb: linear sun 1 color.
    sun_color: vec4<f32>,
    // xyz: dir toward sun 2, w: 1.0 if active else 0.0.
    sun2_dir: vec4<f32>,
    // rgb: linear sun 2 color.
    sun2_color: vec4<f32>,
    // x: user strength scale, y: local cone density, z: sun shaft density, w: max march dist.
    settings: vec4<f32>,
    // x: frame_count, y: light_count (<= 16), z: debug_mode, w: unused.
    counts: vec4<u32>,
    lights: array<VolumetricLight, 16>,
}

@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var<uniform> view: View;
@group(0) @binding(3) var<uniform> vol: VolumetricUniform;

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

fn hg_phase(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    let denom = max(1.0 + g2 - 2.0 * g * cos_theta, 1e-4);
    return (1.0 - g2) / (4.0 * PI * pow(denom, 1.5));
}

// Fast screen-space depth occlusion probe along a sun direction from march point `p`.
fn screen_sun_vis(p: vec3<f32>, sun_dir: vec3<f32>, probe_dist: f32, vp_origin: vec2<f32>, vp_size: vec2<f32>) -> f32 {
    let probe = p + sun_dir * probe_dist;
    let clip_pos = view.clip_from_world * vec4<f32>(probe, 1.0);
    if clip_pos.w <= 1e-4 {
        return 0.65;
    }
    let ndc_xy = clip_pos.xy / clip_pos.w;
    if abs(ndc_xy.x) > 1.05 || abs(ndc_xy.y) > 1.05 {
        return 0.65;
    }
    let uv = clamp(vec2<f32>(ndc_xy.x * 0.5 + 0.5, 0.5 - ndc_xy.y * 0.5), vec2<f32>(0.0), vec2<f32>(1.0));
    let tap = clamp(vec2<i32>(vp_origin + uv * vp_size), vec2<i32>(vol.bounds.xy), vec2<i32>(vol.bounds.zw) - vec2<i32>(1));
    let occ_depth = textureLoad(depth_tex, tap, 0i);
    if occ_depth <= 1e-6 {
        return 1.0;
    }
    let ray_dist = view_distance(clip_pos.z / clip_pos.w);
    let occ_dist = view_distance(occ_depth);
    return smoothstep(0.65, -0.15, ray_dist - occ_dist);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let pixel_u = vec2<u32>(in.position.xy);
    let tex_size_i = vec2<i32>(textureDimensions(scene_tex));
    let pixel_i = clamp(vec2<i32>(pixel_u), vec2<i32>(0), tex_size_i - vec2<i32>(1));
    let src = textureLoad(scene_tex, pixel_i, 0i);
    if any(pixel_u < vol.bounds.xy) || any(pixel_u >= vol.bounds.zw) {
        return src;
    }
    let depth_size_i = vec2<i32>(textureDimensions(depth_tex));
    let depth_i = clamp(vec2<i32>(pixel_u), vec2<i32>(0), depth_size_i - vec2<i32>(1));
    let d = textureLoad(depth_tex, depth_i, 0i);
    let vp_origin = vec2<f32>(vol.bounds.xy);
    let vp_size = max(vec2<f32>(vol.bounds.zw - vol.bounds.xy), vec2<f32>(1.0));
    let frame_uv = (in.position.xy - vp_origin) / vp_size;

    let far_world = world_from_screen(frame_uv, select(d, 0.02, d <= 1e-6));
    let to_pixel = far_world - view.world_position;
    let ray_dir = normalize(to_pixel);
    let scene_dist = select(vol.settings.w, min(view_distance(d), vol.settings.w), d > 1e-6);

    let steps: u32 = 16u;
    let step_len = scene_dist / f32(steps);
    let jitter = ign_noise(in.position.xy, f32(vol.counts.x & 63u));
    let light_count = min(vol.counts.y, VOL_LIGHTS);

    let has_sun1 = vol.sun_dir.w > 0.5;
    let has_sun2 = vol.sun2_dir.w > 0.5;
    let sun1_phase = select(0.0, hg_phase(dot(ray_dir, vol.sun_dir.xyz), 0.68), has_sun1);
    let sun2_phase = select(0.0, hg_phase(dot(ray_dir, vol.sun2_dir.xyz), 0.68), has_sun2);

    var scatter = vec3<f32>(0.0);
    for (var s = 0u; s < steps; s++) {
        let march_t = (f32(s) + 0.2 + 0.8 * jitter) * step_len;
        let p = view.world_position + ray_dir * march_t;
        let dist_fade = 1.0 - smoothstep(vol.settings.w * 0.6, vol.settings.w, march_t);

        if has_sun1 {
            let probe_step = clamp(1.2 + march_t * 0.14, 1.2, 6.0);
            let vis = screen_sun_vis(p, vol.sun_dir.xyz, probe_step, vp_origin, vp_size);
            scatter = scatter + vol.sun_color.rgb * (vis * sun1_phase * step_len * vol.settings.z * dist_fade);
        }
        if has_sun2 {
            let probe_step = clamp(1.2 + march_t * 0.14, 1.2, 6.0);
            let vis2 = screen_sun_vis(p, vol.sun2_dir.xyz, probe_step, vp_origin, vp_size);
            scatter = scatter + vol.sun2_color.rgb * (vis2 * sun2_phase * step_len * vol.settings.z * dist_fade);
        }

        for (var l = 0u; l < min(light_count, VOL_LIGHTS); l++) {
            let light = vol.lights[min(l, VOL_LIGHTS - 1u)];
            let to_light = light.pos_radius.xyz - p;
            let radius = max(light.pos_radius.w, 0.1);
            let dist_sq = dot(to_light, to_light);
            if dist_sq >= radius * radius {
                continue;
            }
            let dist_l = sqrt(dist_sq);
            let l_dir = to_light / max(dist_l, 1e-4);
            let x = dist_l / max(radius * 0.42, 0.15);
            let edge = dist_l / radius;
            let win = saturate(1.0 - edge * edge * edge * edge);
            let atten = (win * win) / (1.0 + x * x);
            var cone = 1.0;
            if light.aim_cone.w > 0.0 {
                let out_cos = dot(light.aim_cone.xyz, -l_dir);
                cone = smoothstep(light.aim_cone.w - 0.35, light.aim_cone.w + 0.12, out_cos);
            }
            let phase = hg_phase(dot(ray_dir, l_dir), 0.42);
            scatter = scatter + light.color_strength.rgb * (atten * cone * phase * step_len * vol.settings.y * dist_fade);
        }
    }

    scatter = min(scatter * vol.settings.x, vec3<f32>(0.42));
    if vol.counts.z != 0u {
        return vec4<f32>(scatter, 1.0);
    }
    return vec4<f32>(src.rgb + scatter, src.a);
}
