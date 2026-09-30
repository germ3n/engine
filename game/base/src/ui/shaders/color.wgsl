struct ScreenUniforms {
    resolution: vec4<f32>,
}

var<immediate> pc: ScreenUniforms;

struct VsIn {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(vin: VsIn) -> VsOut {
    var unit = vin.position / pc.resolution.xy;
    var clip = unit * 2.0 - 1.0;
    clip.y = -clip.y;
    var vout: VsOut;
    vout.clip_position = vec4(clip, 0.0, 1.0);
    vout.color = vin.color;
    return vout;
}

@fragment
fn fs_main(vin: VsOut) -> @location(0) vec4<f32> {
    return vin.color;
}
