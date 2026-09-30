pub mod mesh {
    pub const SOURCE: &str = include_str!("mesh.wgsl");
}

pub mod color {
    pub const SOURCE: &str = include_str!("color.wgsl");
}

pub mod text {
    pub const SOURCE: &str = include_str!("text.wgsl");
}

#[derive(Clone, Copy)]
pub enum Program {
    Mesh,
    Color,
    Text,
}

#[derive(Clone, Copy)]
pub enum Stage {
    Vertex,
    Fragment,
}

#[derive(Clone, Copy)]
pub enum Target {
    Spirv,
    Msl,
    Hlsl,
    Glsl330,
    GlslEs300,
}

pub enum Compiled {
    Spirv(Vec<u32>),
    Source(String),
}

impl Compiled {
    pub fn source(&self) -> &str {
        match self {
            Compiled::Source(text) => text,
            Compiled::Spirv(_) => "",
        }
    }

    pub fn spirv(&self) -> &[u32] {
        match self {
            Compiled::Spirv(words) => words,
            Compiled::Source(_) => &[],
        }
    }
}

impl Program {
    pub fn path(self) -> &'static str {
        match self {
            Program::Mesh => "shaders/mesh.wgsl",
            Program::Color => "shaders/color.wgsl",
            Program::Text => "shaders/text.wgsl",
        }
    }

    pub fn embedded(self) -> &'static str {
        match self {
            Program::Mesh => mesh::SOURCE,
            Program::Color => color::SOURCE,
            Program::Text => text::SOURCE,
        }
    }

    pub fn wgsl(self) -> String {
        match crate::fs::read_string(self.path()) {
            Ok(text) => {
                log::info!(
                    "[shader] loaded {} from fs ({} bytes)",
                    self.path(),
                    text.len()
                );

                text
            }
            Err(err) => {
                let embedded = self.embedded();
                log::warn!(
                    "[shader] {} missing from fs ({err}), using embedded fallback ({} bytes)",
                    self.path(),
                    embedded.len()
                );

                embedded.to_string()
            }
        }
    }
}

impl Stage {
    fn entry(self) -> &'static str {
        match self {
            Stage::Vertex => "vs_main",
            Stage::Fragment => "fs_main",
        }
    }

    fn naga(self) -> naga::ShaderStage {
        match self {
            Stage::Vertex => naga::ShaderStage::Vertex,
            Stage::Fragment => naga::ShaderStage::Fragment,
        }
    }
}

pub fn compile(program: Program, stage: Stage, target: Target) -> Result<Compiled, String> {
    let source = program.wgsl();
    let module = naga::front::wgsl::parse_str(&source).map_err(|err| format!("wgsl: {err}"))?;
    let caps = naga::valid::Capabilities::default() | naga::valid::Capabilities::IMMEDIATES;
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), caps)
        .validate(&module)
        .map_err(|err| format!("validate: {err:?}"))?;

    match target {
        Target::Spirv => {
            let options = naga::back::spv::Options::default();
            let pipeline = naga::back::spv::PipelineOptions {
                shader_stage: stage.naga(),
                entry_point: stage.entry().to_string(),
            };
            let words = naga::back::spv::write_vec(&module, &info, &options, Some(&pipeline))
                .map_err(|err| format!("spirv: {err}"))?;

            Ok(Compiled::Spirv(words))
        }
        Target::Msl => {
            let mut options = naga::back::msl::Options::default();
            options.lang_version = (2, 1);
            options
                .per_entry_point_map
                .insert(stage.entry().to_string(), {
                    let mut resources = naga::back::msl::EntryPointResources::default();
                    resources.immediates_buffer = Some(1);
                    if matches!(program, Program::Text) {
                        resources.resources.insert(
                            naga::ResourceBinding {
                                group: 0,
                                binding: 0,
                            },
                            naga::back::msl::BindTarget {
                                texture: Some(0),
                                ..Default::default()
                            },
                        );
                        resources.resources.insert(
                            naga::ResourceBinding {
                                group: 0,
                                binding: 1,
                            },
                            naga::back::msl::BindTarget {
                                sampler: Some(naga::back::msl::BindSamplerTarget::Resource(0)),
                                ..Default::default()
                            },
                        );
                    }

                    resources
                });
            let (source, _) = naga::back::msl::write_string(
                &module,
                &info,
                &options,
                &naga::back::msl::PipelineOptions::default(),
            )
            .map_err(|err| format!("msl: {err:?}"))?;

            Ok(Compiled::Source(source))
        }
        Target::Hlsl => {
            let mut options = naga::back::hlsl::Options::default();
            options.shader_model = naga::back::hlsl::ShaderModel::V5_0;
            options.immediates_target = Some(naga::back::hlsl::BindTarget {
                space: 0,
                register: 0,
                binding_array_size: None,
                dynamic_storage_buffer_offsets_index: None,
                restrict_indexing: false,
            });
            let pipeline = naga::back::hlsl::PipelineOptions::default();
            let mut output = String::new();
            let mut writer = naga::back::hlsl::Writer::new(&mut output, &options, &pipeline);
            writer
                .write(&module, &info, None)
                .map_err(|err| format!("hlsl: {err:?}"))?;

            Ok(Compiled::Source(output))
        }
        Target::Glsl330 | Target::GlslEs300 => {
            let version = match target {
                Target::Glsl330 => naga::back::glsl::Version::Desktop(330),
                Target::GlslEs300 => naga::back::glsl::Version::Embedded {
                    version: 300,
                    is_webgl: false,
                },
                _ => unreachable!(),
            };
            let mut options = naga::back::glsl::Options::default();
            options.version = version;
            let pipeline = naga::back::glsl::PipelineOptions {
                shader_stage: stage.naga(),
                entry_point: stage.entry().to_string(),
                multiview: None,
            };
            let mut output = String::new();
            let mut writer = naga::back::glsl::Writer::new(
                &mut output,
                &module,
                &info,
                &options,
                &pipeline,
                naga::proc::BoundsCheckPolicies::default(),
            )
            .map_err(|err| format!("glsl: {err:?}"))?;
            writer
                .write()
                .map_err(|err| format!("glsl write: {err:?}"))?;

            Ok(Compiled::Source(output))
        }
    }
}

pub fn spirv(program: Program, stage: Stage) -> Result<Vec<u32>, String> {
    match compile(program, stage, Target::Spirv)? {
        Compiled::Spirv(words) => Ok(words),
        Compiled::Source(_) => Err("spirv".to_string()),
    }
}

pub fn source(program: Program, stage: Stage, target: Target) -> Result<String, String> {
    match compile(program, stage, target)? {
        Compiled::Source(text) => Ok(text),
        Compiled::Spirv(_) => Err("source".to_string()),
    }
}

pub fn msl_library(program: Program) -> Result<String, String> {
    let wgsl = program.wgsl();
    let module = naga::front::wgsl::parse_str(&wgsl).map_err(|err| format!("wgsl: {err}"))?;
    let caps = naga::valid::Capabilities::default() | naga::valid::Capabilities::IMMEDIATES;
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), caps)
        .validate(&module)
        .map_err(|err| format!("validate: {err:?}"))?;
    let mut options = naga::back::msl::Options::default();
    options.lang_version = (2, 1);

    for entry in ["vs_main", "fs_main"] {
        let mut resources = naga::back::msl::EntryPointResources::default();
        resources.immediates_buffer = Some(1);

        if matches!(program, Program::Text) {
            resources.resources.insert(
                naga::ResourceBinding {
                    group: 0,
                    binding: 0,
                },
                naga::back::msl::BindTarget {
                    texture: Some(0),
                    ..Default::default()
                },
            );
            resources.resources.insert(
                naga::ResourceBinding {
                    group: 0,
                    binding: 1,
                },
                naga::back::msl::BindTarget {
                    sampler: Some(naga::back::msl::BindSamplerTarget::Resource(0)),
                    ..Default::default()
                },
            );
        }

        options
            .per_entry_point_map
            .insert(entry.to_string(), resources);
    }

    let (source, _) = naga::back::msl::write_string(
        &module,
        &info,
        &options,
        &naga::back::msl::PipelineOptions::default(),
    )
    .map_err(|err| format!("msl: {err:?}"))?;

    Ok(source)
}

pub fn hlsl(program: Program) -> Result<String, String> {
    source(program, Stage::Vertex, Target::Hlsl)
}
