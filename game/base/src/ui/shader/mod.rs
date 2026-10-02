use naga::back;
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::{Module, ShaderStage};

mod cache;

pub use cache::{id_from_bytes, id_from_luid, id_from_text, id_from_u64, Registry};

pub const MESH: &str = r#"
struct Constants {
    view_proj: mat4x4<f32>,
}

var<immediate> constants: Constants;

struct MeshIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
}

struct MeshVertOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec3<f32>,
}

@vertex
fn mesh_vert(input: MeshIn) -> MeshVertOut {
    var out: MeshVertOut;
    out.position = constants.view_proj * vec4<f32>(input.position, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn mesh_frag(input: MeshVertOut) -> @location(0) vec4<f32> {
    return vec4<f32>(input.color, 1.0);
}
"#;

pub const COLOR: &str = r#"
struct Constants {
    resolution: vec4<f32>,
}

var<immediate> constants: Constants;

struct ColorIn {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct ColorVertOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn color_vert(input: ColorIn) -> ColorVertOut {
    let unit = input.position / constants.resolution.xy;
    var clip = unit * 2.0 - 1.0;
    clip.y = -clip.y;
    var out: ColorVertOut;
    out.position = vec4<f32>(clip, 0.0, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn color_frag(input: ColorVertOut) -> @location(0) vec4<f32> {
    return input.color;
}
"#;

pub const TEXT: &str = r#"
struct Constants {
    resolution: vec4<f32>,
}

var<immediate> constants: Constants;

@group(0) @binding(0) var atlas_tex: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;

struct TextIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

struct TextVertOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn text_vert(input: TextIn) -> TextVertOut {
    let unit = input.position / constants.resolution.xy;
    var clip = unit * 2.0 - 1.0;
    clip.y = -clip.y;
    var out: TextVertOut;
    out.position = vec4<f32>(clip, 0.0, 1.0);
    out.uv = input.uv;
    out.color = input.color;
    return out;
}

@fragment
fn text_frag(input: TextVertOut) -> @location(0) vec4<f32> {
    let coverage = textureSample(atlas_tex, atlas_sampler, input.uv).r;
    return vec4<f32>(input.color.rgb, input.color.a * coverage);
}
"#;

pub const UI: &str = r#"
struct Constants {
    resolution: vec2<f32>,
    color: vec4<f32>,
}

var<immediate> constants: Constants;

@vertex
fn ui_vert(@location(0) position: vec2<f32>) -> @builtin(position) vec4<f32> {
    let unit = position / constants.resolution;
    var clip = unit * 2.0 - 1.0;
    clip.y = -clip.y;
    return vec4<f32>(clip, 0.0, 1.0);
}

@fragment
fn ui_frag() -> @location(0) vec4<f32> {
    return constants.color;
}
"#;

fn parse(source: &str) -> Result<(Module, naga::valid::ModuleInfo), String> {
    let module = naga::front::wgsl::parse_str(source).map_err(|err| err.to_string())?;
    let caps = Capabilities::default() | Capabilities::IMMEDIATES;
    let info = Validator::new(ValidationFlags::all(), caps)
        .validate(&module)
        .map_err(|err| format!("{err:?}"))?;

    Ok((module, info))
}

fn bind_target(register: u32) -> back::hlsl::BindTarget {
    back::hlsl::BindTarget {
        space: 0,
        register,
        binding_array_size: None,
        dynamic_storage_buffer_offsets_index: None,
        restrict_indexing: false,
    }
}

fn fixup_hlsl(source: &str) -> String {
    let mut out = String::with_capacity(source.len());

    for line in source.lines() {
        if line.contains("nagaSamplerHeap")
            || line.contains("nagaComparisonSamplerHeap")
            || line.contains("nagaGroup") && line.contains("SamplerIndexArray")
        {
            continue;
        }

        if line.contains("ConstantBuffer<Constants> constants: register(b0);") {
            out.push_str("cbuffer constants_buf : register(b0) { Constants constants; };\n");
            continue;
        }

        if line.contains("static const SamplerState atlas_sampler") {
            out.push_str("SamplerState atlas_sampler : register(s0);\n");
            continue;
        }

        out.push_str(line);
        out.push('\n');
    }

    out
}

pub fn spirv(source: &str, stage: ShaderStage, entry: &str) -> Result<Vec<u32>, String> {
    let (module, info) = parse(source)?;
    let options = back::spv::Options::default();
    let pipeline = back::spv::PipelineOptions {
        shader_stage: stage,
        entry_point: entry.to_string(),
    };

    back::spv::write_vec(&module, &info, &options, Some(&pipeline)).map_err(|err| err.to_string())
}

pub fn hlsl(source: &str) -> Result<String, String> {
    let (module, info) = parse(source)?;
    let mut options = back::hlsl::Options::default();
    options.shader_model = back::hlsl::ShaderModel::V5_0;
    options.immediates_target = Some(bind_target(0));
    options.binding_map.insert(
        naga::ResourceBinding {
            group: 0,
            binding: 0,
        },
        bind_target(0),
    );
    options.binding_map.insert(
        naga::ResourceBinding {
            group: 0,
            binding: 1,
        },
        bind_target(0),
    );

    let pipeline = back::hlsl::PipelineOptions::default();
    let mut out = String::new();
    let mut writer = back::hlsl::Writer::new(&mut out, &options, &pipeline);
    writer
        .write(&module, &info, None)
        .map_err(|err| err.to_string())?;

    Ok(fixup_hlsl(&out))
}

pub fn msl(source: &str) -> Result<String, String> {
    let (module, info) = parse(source)?;
    let mut options = back::msl::Options {
        lang_version: (2, 0),
        fake_missing_bindings: false,
        ..back::msl::Options::default()
    };
    options
        .inline_samplers
        .push(back::msl::sampler::InlineSampler {
            mag_filter: back::msl::sampler::Filter::Linear,
            min_filter: back::msl::sampler::Filter::Linear,
            ..back::msl::sampler::InlineSampler::default()
        });

    for entry in &module.entry_points {
        let mut resources = back::msl::EntryPointResources::default();
        resources.immediates_buffer = Some(if source.contains("SkinUniforms") {
            2
        } else {
            1
        });
        resources.resources.insert(
            naga::ResourceBinding {
                group: 0,
                binding: 0,
            },
            back::msl::BindTarget {
                texture: Some(0),
                ..back::msl::BindTarget::default()
            },
        );
        resources.resources.insert(
            naga::ResourceBinding {
                group: 0,
                binding: 1,
            },
            back::msl::BindTarget {
                sampler: Some(back::msl::BindSamplerTarget::Inline(0)),
                ..back::msl::BindTarget::default()
            },
        );
        resources.resources.insert(
            naga::ResourceBinding {
                group: 0,
                binding: 2,
            },
            back::msl::BindTarget {
                texture: Some(1),
                ..back::msl::BindTarget::default()
            },
        );
        options
            .per_entry_point_map
            .insert(entry.name.clone(), resources);
    }

    let (out, _) = back::msl::write_string(
        &module,
        &info,
        &options,
        &back::msl::PipelineOptions::default(),
    )
    .map_err(|err| err.to_string())?;

    Ok(out)
}

pub fn glsl(
    source: &str,
    stage: ShaderStage,
    entry: &str,
    version: back::glsl::Version,
) -> Result<(String, back::glsl::ReflectionInfo), String> {
    let (module, info) = parse(source)?;
    let options = back::glsl::Options {
        version,
        writer_flags: back::glsl::WriterFlags::empty(),
        binding_map: Default::default(),
        zero_initialize_workgroup_memory: true,
    };
    let pipeline = back::glsl::PipelineOptions {
        shader_stage: stage,
        entry_point: entry.to_string(),
        multiview: None,
    };
    let mut out = String::new();
    let mut writer = back::glsl::Writer::new(
        &mut out,
        &module,
        &info,
        &options,
        &pipeline,
        naga::proc::BoundsCheckPolicies::default(),
    )
    .map_err(|err| err.to_string())?;
    let reflection = writer.write().map_err(|err| err.to_string())?;

    Ok((out, reflection))
}

pub fn glsl_version() -> back::glsl::Version {
    #[cfg(target_os = "android")]
    {
        back::glsl::Version::Embedded {
            version: 300,
            is_webgl: false,
        }
    }

    #[cfg(not(target_os = "android"))]
    {
        back::glsl::Version::Desktop(330)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaders_translate() {
        for (source, vert, frag) in [
            (MESH, "mesh_vert", "mesh_frag"),
            (COLOR, "color_vert", "color_frag"),
            (TEXT, "text_vert", "text_frag"),
            (UI, "ui_vert", "ui_frag"),
        ] {
            let words = spirv(source, ShaderStage::Vertex, vert).unwrap();
            assert_eq!(words[0], 0x07230203);
            spirv(source, ShaderStage::Fragment, frag).unwrap();
            let hlsl = hlsl(source).unwrap();
            assert!(hlsl.contains(vert), "{vert} missing from hlsl:\n{hlsl}");
            assert!(hlsl.contains(frag), "{frag} missing from hlsl:\n{hlsl}");
            assert!(
                !hlsl.contains("nagaSamplerHeap"),
                "sampler heap left in hlsl:\n{hlsl}"
            );
            assert!(
                !hlsl.contains("ConstantBuffer<"),
                "ConstantBuffer left in hlsl:\n{hlsl}"
            );
            let msl = msl(source).unwrap();
            assert!(msl.contains(vert), "{vert} missing from msl:\n{msl}");
            assert!(msl.contains(frag), "{frag} missing from msl:\n{msl}");
            let (glsl_vert, _) = glsl(source, ShaderStage::Vertex, vert, glsl_version()).unwrap();
            assert!(glsl_vert.contains("#version"));
            let (glsl_frag, _) = glsl(source, ShaderStage::Fragment, frag, glsl_version()).unwrap();
            assert!(glsl_frag.contains("#version"));
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    fn metal_shaders_compile() {
        use metal::*;

        let device = Device::system_default().expect("metal");

        for source in [MESH, COLOR, TEXT] {
            let msl = msl(source).unwrap();
            device
                .new_library_with_source(&msl, &CompileOptions::new())
                .unwrap_or_else(|err| panic!("msl compile failed: {err}\n{msl}"));
        }
    }
}
