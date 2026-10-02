struct ViewGpu {
    view_proj: mat4x4<f32>,
    eye_time: vec4<f32>,
    screen: vec4<f32>,
}

var<immediate> view: ViewGpu;

struct MaterialGpu {
    tint: vec4<f32>,
    params: vec4<f32>,
    detail: vec4<f32>,
    env: vec4<f32>,
    extra: vec4<f32>,
    fog: vec4<f32>,
}

@group(0) @binding(0) var<uniform> material: MaterialGpu;
@group(0) @binding(1) var wrap_samp: sampler;
@group(0) @binding(2) var base_tex: texture_2d<f32>;
@group(0) @binding(3) var base2_tex: texture_2d<f32>;
@group(0) @binding(4) var bump_tex: texture_2d<f32>;
@group(0) @binding(5) var bump2_tex: texture_2d<f32>;
@group(0) @binding(6) var detail_tex: texture_2d<f32>;
@group(0) @binding(7) var blend_tex: texture_2d<f32>;
@group(0) @binding(8) var mask_tex: texture_2d<f32>;
@group(0) @binding(9) var env_tex: texture_cube<f32>;
@group(1) @binding(0) var light0_tex: texture_2d<f32>;
@group(1) @binding(1) var light1_tex: texture_2d<f32>;
@group(1) @binding(2) var light2_tex: texture_2d<f32>;
@group(1) @binding(3) var light3_tex: texture_2d<f32>;
@group(1) @binding(4) var scene_tex: texture_2d<f32>;
@group(1) @binding(5) var clamp_samp: sampler;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) light_uv: vec2<f32>,
    @location(5) color: vec3<f32>,
    @location(6) blend: f32,
    @location(7) material_id: f32,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) light_uv: vec2<f32>,
    @location(2) color: vec3<f32>,
    @location(3) normal: vec3<f32>,
    @location(4) tangent: vec4<f32>,
    @location(5) world: vec3<f32>,
    @location(6) blend: f32,
    @location(7) material_id: f32,
}

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = view.view_proj * vec4<f32>(input.position, 1.0);

    if (view.screen.z > 0.5) {
        out.clip.z = out.clip.w;
    }

    out.uv = input.uv;
    out.light_uv = input.light_uv;
    out.color = input.color;
    out.normal = input.normal;
    out.tangent = input.tangent;
    out.world = input.position;
    out.blend = input.blend;
    out.material_id = input.material_id;

    return out;
}

fn saturate(value: f32) -> f32 {
    return clamp(value, 0.0, 1.0);
}

fn tangent_basis(normal: vec3<f32>, tangent: vec4<f32>) -> mat3x3<f32> {
    let n = normalize(normal);
    let t = normalize(tangent.xyz);
    let b = normalize(cross(n, t)) * tangent.w;

    return mat3x3<f32>(t, b, n);
}

fn surface_normal(input: VsOut) -> vec3<f32> {
    let flags = bitcast<u32>(material.detail.w);
    let geometric = normalize(input.normal);

    if ((flags & 1u) == 0u) {
        return geometric;
    }

    let sample = textureSample(bump_tex, wrap_samp, input.uv);
    var local = sample.rgb * 2.0 - vec3<f32>(1.0);

    if ((flags & 2u) != 0u) {
        let b0 = vec3<f32>(0.8164966, 0.0, 0.5773503);
        let b1 = vec3<f32>(-0.4082483, 0.7071068, 0.5773503);
        let b2 = vec3<f32>(-0.4082483, -0.7071068, 0.5773503);
        local = normalize(sample.r * b0 + sample.g * b1 + sample.b * b2);
    }

    if ((flags & 1024u) != 0u) {
        let second = textureSample(bump2_tex, wrap_samp, input.uv).rgb * 2.0 - vec3<f32>(1.0);
        local = normalize(mix(local, second, saturate(input.blend)));
    }

    return normalize(tangent_basis(geometric, input.tangent) * local);
}

fn light_color(input: VsOut, normal: vec3<f32>) -> vec3<f32> {
    let flags = bitcast<u32>(material.detail.w);
    let direct = textureSample(light0_tex, clamp_samp, input.light_uv).rgb;

    if ((flags & 1u) == 0u) {
        return direct;
    }

    let l1 = textureSample(light1_tex, clamp_samp, input.light_uv).rgb;
    let l2 = textureSample(light2_tex, clamp_samp, input.light_uv).rgb;
    let l3 = textureSample(light3_tex, clamp_samp, input.light_uv).rgb;
    let b0 = vec3<f32>(0.8164966, 0.0, 0.5773503);
    let b1 = vec3<f32>(-0.4082483, 0.7071068, 0.5773503);
    let b2 = vec3<f32>(-0.4082483, -0.7071068, 0.5773503);
    var weight = vec3<f32>(dot(normal, b0), dot(normal, b1), dot(normal, b2));
    weight = max(weight, vec3<f32>(0.0));
    weight = weight * weight;
    let sum = max(weight.x + weight.y + weight.z, 0.001);

    return (weight.x * l1 + weight.y * l2 + weight.z * l3) / sum;
}

fn apply_detail(base: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let flags = bitcast<u32>(material.detail.w);

    if ((flags & 4u) == 0u) {
        return base;
    }

    let sample = textureSample(detail_tex, wrap_samp, uv * material.detail.xy);
    let mode = u32(material.params.z + 0.5);
    let factor = material.detail.z;

    if (mode == 1u) {
        return base + sample.rgb * factor;
    }

    if (mode == 2u) {
        return mix(base, sample.rgb, sample.a * factor);
    }

    if (mode == 4u) {
        return base * mix(vec3<f32>(1.0), sample.rgb, factor);
    }

    return mix(base, base * sample.rgb * 2.0, factor);
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let flags = bitcast<u32>(material.detail.w);
    let mode = u32(material.params.x + 0.5);
    var albedo = textureSample(base_tex, wrap_samp, input.uv);

    if (mode == 2u) {
        let second = textureSample(base2_tex, wrap_samp, input.uv);
        var weight = saturate(input.blend);

        if ((flags & 256u) != 0u) {
            weight = weight * textureSample(blend_tex, wrap_samp, input.uv).r;
        }

        albedo = vec4<f32>(mix(albedo.rgb, second.rgb, weight), albedo.a);
    }

    if ((flags & 64u) != 0u && albedo.a < material.params.y) {
        discard;
    }

    let alpha = albedo.a * material.params.w * material.tint.w;
    let view_dir = normalize(view.eye_time.xyz - input.world);
    let normal = surface_normal(input);

    if (mode == 0u) {
        let dir = normalize(input.world - view.eye_time.xyz);
        let sky = textureSample(env_tex, wrap_samp, dir).rgb;

        return vec4<f32>(sky * input.color * material.tint.rgb, 1.0);
    }

    if (mode == 3u) {
        let screen = input.clip.xy / max(view.screen.xy, vec2<f32>(1.0));
        let refracted = textureSample(scene_tex, clamp_samp, screen + normal.xy * 0.03).rgb;
        let reflected = textureSample(env_tex, wrap_samp, reflect(-view_dir, normal)).rgb;
        let fresnel = pow(1.0 - saturate(dot(normal, view_dir)), max(material.fog.w, 0.5));
        var water = mix(refracted, reflected * material.env.rgb, fresnel * 0.7);
        water = mix(water, material.fog.rgb, 0.25);
        let scroll = textureSample(bump_tex, wrap_samp, input.uv + material.extra.yz * view.eye_time.w).rgb;
        water = water * (0.75 + scroll * 0.25);

        return vec4<f32>(water * material.tint.rgb, alpha);
    }

    var rgb = albedo.rgb * input.color * material.tint.rgb;

    if (mode == 1u || mode == 2u) {
        rgb = rgb * light_color(input, normal);
        rgb = apply_detail(rgb, input.uv);
    }

    if ((flags & 8u) != 0u) {
        var mask = 1.0;

        if ((flags & 512u) != 0u) {
            mask = textureSample(mask_tex, wrap_samp, input.uv).r;
        }

        let reflected = textureSample(env_tex, wrap_samp, reflect(-view_dir, normal)).rgb;
        rgb = rgb + reflected * material.env.rgb * mask;
    }

    if ((flags & 128u) != 0u) {
        let light_dir = normalize(vec3<f32>(0.35, 0.45, 0.82));
        let half_dir = normalize(light_dir + view_dir);
        let spec = pow(saturate(dot(normal, half_dir)), max(material.extra.x, 1.0));
        rgb = rgb + spec * material.env.rgb * material.env.w;
    }

    if ((flags & 32u) != 0u) {
        rgb = rgb + albedo.rgb * material.extra.w;
    }

    if (mode == 5u) {
        rgb = albedo.rgb * material.tint.rgb;
    }

    return vec4<f32>(rgb, alpha);
}

@group(2) @binding(0) var batch_base0: texture_2d_array<f32>;
@group(2) @binding(1) var batch_base1: texture_2d_array<f32>;
@group(2) @binding(2) var batch_bump0: texture_2d_array<f32>;
@group(2) @binding(3) var batch_bump1: texture_2d_array<f32>;
@group(2) @binding(4) var batch_params: texture_2d<f32>;
@group(2) @binding(5) var batch_detail0: texture_2d_array<f32>;
@group(2) @binding(6) var batch_detail1: texture_2d_array<f32>;
@group(2) @binding(7) var batch_base20: texture_2d_array<f32>;
@group(2) @binding(8) var batch_base21: texture_2d_array<f32>;
@group(2) @binding(9) var batch_env: texture_cube_array<f32>;

fn batch_row(id: f32) -> i32 {
    return i32(id + 0.5) % 65536;
}

fn batch_field(id: f32, column: i32) -> vec4<f32> {
    return textureLoad(batch_params, vec2<i32>(column, batch_row(id)), 0);
}

fn batch_base(slot: u32, layer: i32, uv: vec2<f32>) -> vec4<f32> {
    if (slot == 0u) {
        return textureSample(batch_base0, wrap_samp, uv, layer);
    }

    return textureSample(batch_base1, wrap_samp, uv, layer);
}

fn batch_bump(slot: u32, layer: i32, uv: vec2<f32>) -> vec4<f32> {
    if (slot == 0u) {
        return textureSample(batch_bump0, wrap_samp, uv, layer);
    }

    return textureSample(batch_bump1, wrap_samp, uv, layer);
}

fn batch_detail(slot: u32, layer: i32, uv: vec2<f32>) -> vec4<f32> {
    if (slot == 0u) {
        return textureSample(batch_detail0, wrap_samp, uv, layer);
    }

    return textureSample(batch_detail1, wrap_samp, uv, layer);
}

fn batch_base2(slot: u32, layer: i32, uv: vec2<f32>) -> vec4<f32> {
    if (slot == 0u) {
        return textureSample(batch_base20, wrap_samp, uv, layer);
    }

    return textureSample(batch_base21, wrap_samp, uv, layer);
}

fn batch_normal(input: VsOut, flags: u32, maps: vec4<f32>) -> vec3<f32> {
    let geometric = normalize(input.normal);

    if ((flags & 1u) == 0u) {
        return geometric;
    }

    let sample = batch_bump(u32(maps.z + 0.5), i32(maps.w + 0.5), input.uv);
    var local = sample.rgb * 2.0 - vec3<f32>(1.0);

    if ((flags & 2u) != 0u) {
        let b0 = vec3<f32>(0.8164966, 0.0, 0.5773503);
        let b1 = vec3<f32>(-0.4082483, 0.7071068, 0.5773503);
        let b2 = vec3<f32>(-0.4082483, -0.7071068, 0.5773503);
        local = normalize(sample.r * b0 + sample.g * b1 + sample.b * b2);
    }

    return normalize(tangent_basis(geometric, input.tangent) * local);
}

fn batch_light(input: VsOut, flags: u32, normal: vec3<f32>) -> vec3<f32> {
    let direct = textureSample(light0_tex, clamp_samp, input.light_uv).rgb;

    if ((flags & 1u) == 0u) {
        return direct;
    }

    let l1 = textureSample(light1_tex, clamp_samp, input.light_uv).rgb;
    let l2 = textureSample(light2_tex, clamp_samp, input.light_uv).rgb;
    let l3 = textureSample(light3_tex, clamp_samp, input.light_uv).rgb;
    let b0 = vec3<f32>(0.8164966, 0.0, 0.5773503);
    let b1 = vec3<f32>(-0.4082483, 0.7071068, 0.5773503);
    let b2 = vec3<f32>(-0.4082483, -0.7071068, 0.5773503);
    var weight = vec3<f32>(dot(normal, b0), dot(normal, b1), dot(normal, b2));
    weight = max(weight, vec3<f32>(0.0));
    weight = weight * weight;
    let sum = max(weight.x + weight.y + weight.z, 0.001);

    return (weight.x * l1 + weight.y * l2 + weight.z * l3) / sum;
}

@fragment
fn fs_batch(input: VsOut) -> @location(0) vec4<f32> {
    let tint = batch_field(input.material_id, 0);
    let params = batch_field(input.material_id, 1);
    let detail = batch_field(input.material_id, 2);
    let env = batch_field(input.material_id, 3);
    let extra = batch_field(input.material_id, 4);
    let maps = batch_field(input.material_id, 6);
    let extra_maps = batch_field(input.material_id, 7);
    let flags = bitcast<u32>(detail.w);
    let mode = u32(params.x + 0.5);
    var albedo = batch_base(u32(maps.x + 0.5), i32(maps.y + 0.5), input.uv);

    if (mode == 2u) {
        let second = batch_base2(u32(extra_maps.z + 0.5), i32(extra_maps.w + 0.5), input.uv);
        var weight = saturate(input.blend);

        if ((flags & 256u) != 0u) {
            weight = weight * batch_detail(u32(extra_maps.x + 0.5), i32(extra_maps.y + 0.5), input.uv).r;
        }

        albedo = vec4<f32>(mix(albedo.rgb, second.rgb, weight), albedo.a);
    }

    if ((flags & 64u) != 0u && albedo.a < params.y) {
        discard;
    }

    let alpha = albedo.a * params.w * tint.w;

    if (mode == 0u) {
        return vec4<f32>(albedo.rgb * input.color * tint.rgb, 1.0);
    }

    let normal = batch_normal(input, flags, maps);
    var rgb = albedo.rgb * input.color * tint.rgb * batch_light(input, flags, normal);

    if ((flags & 4u) != 0u) {
        let detail_sample = batch_detail(
            u32(extra_maps.x + 0.5),
            i32(extra_maps.y + 0.5),
            input.uv * detail.xy,
        );
        let factor = detail.z;
        let detail_mode = u32(params.z + 0.5);

        if (detail_mode == 1u) {
            rgb = rgb + detail_sample.rgb * factor;
        } else if (detail_mode == 2u) {
            rgb = mix(rgb, detail_sample.rgb, detail_sample.a * factor);
        } else if (detail_mode == 4u) {
            rgb = rgb * mix(vec3<f32>(1.0), detail_sample.rgb, factor);
        } else {
            rgb = mix(rgb, rgb * detail_sample.rgb * 2.0, factor);
        }
    }

    if ((flags & 8u) != 0u) {
        let view_dir = normalize(view.eye_time.xyz - input.world);
        let cube = i32(input.material_id + 0.5) / 65536;
        let reflected = textureSample(batch_env, wrap_samp, reflect(-view_dir, normal), cube).rgb;
        rgb = rgb + reflected * env.rgb;
    }

    if ((flags & 128u) != 0u) {
        let view_dir = normalize(view.eye_time.xyz - input.world);
        let light_dir = normalize(vec3<f32>(0.35, 0.45, 0.82));
        let half_dir = normalize(light_dir + view_dir);
        let spec = pow(saturate(dot(normal, half_dir)), max(extra.x, 1.0));
        rgb = rgb + spec * env.rgb * env.w;
    }

    if ((flags & 32u) != 0u) {
        rgb = rgb + albedo.rgb * extra.w;
    }

    return vec4<f32>(rgb, alpha);
}
