struct MeshUniforms {
    view_proj: mat4x4<f32>,
}

var<immediate> pc: MeshUniforms;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
}

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
}

@vertex
fn vs_main(vin: VsIn) -> VsOut {
    var vout: VsOut;
    vout.clip_position = pc.view_proj * vec4(vin.position, 1.0);
    vout.color = vin.color;
    return vout;
}

@fragment
fn fs_main(vin: VsOut) -> @location(0) vec4<f32> {
    return vec4(vin.color, 1.0);
}
