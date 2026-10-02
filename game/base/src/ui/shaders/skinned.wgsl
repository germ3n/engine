struct SkinUniforms {
    view_proj: mat4x4<f32>,
    bones: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

var<immediate> pc: SkinUniforms;

@group(0) @binding(0) var albedo: texture_2d<f32>;
@group(0) @binding(1) var albedo_samp: sampler;
@group(0) @binding(2) var palette: texture_2d<f32>;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) joints: vec4<f32>,
    @location(4) weights: vec4<f32>,
    @location(5) world0: vec4<f32>,
    @location(6) world1: vec4<f32>,
    @location(7) world2: vec4<f32>,
    @location(8) world3: vec4<f32>,
    @builtin(instance_index) instance: u32,
}

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) normal: vec3<f32>,
}

fn bone_row(instance: u32, bone: u32, row: u32) -> vec4<f32> {
    let x = i32(bone * 3u + row);
    let y = i32(instance);

    return textureLoad(palette, vec2<i32>(x, y), 0);
}

@vertex
fn vs_main(vin: VsIn) -> VsOut {
    var skinned = vec3<f32>(0.0);
    var nrm = vec3<f32>(0.0);
    let point = vec4<f32>(vin.position, 1.0);
    var idx = 0u;

    while idx < 4u {
        let bone = u32(vin.joints[idx]);
        let weight = vin.weights[idx];
        let row0 = bone_row(vin.instance, bone, 0u);
        let row1 = bone_row(vin.instance, bone, 1u);
        let row2 = bone_row(vin.instance, bone, 2u);
        skinned = skinned + weight * vec3<f32>(dot(row0, point), dot(row1, point), dot(row2, point));
        nrm = nrm + weight * vec3<f32>(
            dot(row0.xyz, vin.normal),
            dot(row1.xyz, vin.normal),
            dot(row2.xyz, vin.normal),
        );
        idx = idx + 1u;
    }

    let world = mat4x4<f32>(vin.world0, vin.world1, vin.world2, vin.world3);
    var vout: VsOut;
    vout.clip_position = pc.view_proj * world * vec4<f32>(skinned, 1.0);
    vout.uv = vin.uv;
    vout.normal = nrm;

    return vout;
}

@fragment
fn fs_main(vin: VsOut) -> @location(0) vec4<f32> {
    let color = textureSample(albedo, albedo_samp, vin.uv);
    let lit = normalize(vin.normal);
    let shade = 0.35 + 0.65 * clamp(dot(lit, normalize(vec3<f32>(0.35, 0.45, 0.8))), 0.0, 1.0);

    return vec4<f32>(color.rgb * shade, color.a);
}
