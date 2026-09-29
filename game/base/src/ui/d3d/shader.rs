use windows::core::{s, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, ID3DInclude};

pub const MESH_SM5: &str = r#"
cbuffer Constants : register(b0)
{
    float4x4 view_proj;
};

struct MeshIn
{
    float3 position : POSITION;
    float3 color : COLOR;
};

struct MeshOut
{
    float4 position : SV_Position;
    float3 color : COLOR;
};

MeshOut mesh_vert(MeshIn input)
{
    MeshOut output;
    output.position = mul(view_proj, float4(input.position, 1.0));
    output.color = input.color;
    return output;
}

float4 mesh_frag(MeshOut input) : SV_Target
{
    return float4(input.color, 1.0);
}
"#;

pub const COLOR_SM5: &str = r#"
cbuffer Constants : register(b0)
{
    float4 resolution;
    float4 unused0;
    float4 unused1;
    float4 unused2;
};

struct ColorIn
{
    float2 position : POSITION;
    float4 color : COLOR;
};

struct ColorOut
{
    float4 position : SV_Position;
    float4 color : COLOR;
};

ColorOut color_vert(ColorIn input)
{
    float2 unit = input.position / resolution.xy;
    float2 clip = unit * 2.0 - 1.0;
    clip.y = -clip.y;
    ColorOut output;
    output.position = float4(clip, 0.0, 1.0);
    output.color = input.color;
    return output;
}

float4 color_frag(ColorOut input) : SV_Target
{
    return input.color;
}
"#;

pub const TEXT_SM5: &str = r#"
cbuffer Constants : register(b0)
{
    float4 resolution;
    float4 unused0;
    float4 unused1;
    float4 unused2;
};

struct TextIn
{
    float2 position : POSITION;
    float2 uv : TEXCOORD;
    float4 color : COLOR;
};

struct TextOut
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD;
    float4 color : COLOR;
};

TextOut text_vert(TextIn input)
{
    float2 unit = input.position / resolution.xy;
    float2 clip = unit * 2.0 - 1.0;
    clip.y = -clip.y;
    TextOut output;
    output.position = float4(clip, 0.0, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

Texture2D atlas : register(t0);
SamplerState samp : register(s0);

float4 text_frag(TextOut input) : SV_Target
{
    float coverage = atlas.Sample(samp, input.uv).r;
    return float4(input.color.rgb, input.color.a * coverage);
}
"#;

pub const MESH_SM3: &str = r#"
float4x4 view_proj : register(c0);

struct MeshIn
{
    float3 position : POSITION;
    float3 color : COLOR0;
};

struct MeshOut
{
    float4 position : POSITION;
    float3 color : COLOR0;
};

MeshOut main(MeshIn input)
{
    MeshOut output;
    output.position = mul(view_proj, float4(input.position, 1.0));
    output.color = input.color;
    return output;
}
"#;

pub const MESH_PS_SM3: &str = r#"
float4 main(float3 color : COLOR0) : COLOR
{
    return float4(color, 1.0);
}
"#;

pub const COLOR_SM3: &str = r#"
float4 resolution : register(c0);

struct ColorIn
{
    float2 position : POSITION;
    float4 color : COLOR0;
};

struct ColorOut
{
    float4 position : POSITION;
    float4 color : COLOR0;
};

ColorOut main(ColorIn input)
{
    float2 pixel = input.position - 0.5;
    float2 unit = pixel / resolution.xy;
    float2 clip = unit * 2.0 - 1.0;
    clip.y = -clip.y;
    ColorOut output;
    output.position = float4(clip, 0.0, 1.0);
    output.color = input.color;
    return output;
}
"#;

pub const COLOR_PS_SM3: &str = r#"
float4 main(float4 color : COLOR0) : COLOR
{
    return color;
}
"#;

pub const TEXT_SM3: &str = r#"
float4 resolution : register(c0);

struct TextIn
{
    float2 position : POSITION;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

struct TextOut
{
    float4 position : POSITION;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

TextOut main(TextIn input)
{
    float2 pixel = input.position - 0.5;
    float2 unit = pixel / resolution.xy;
    float2 clip = unit * 2.0 - 1.0;
    clip.y = -clip.y;
    TextOut output;
    output.position = float4(clip, 0.0, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}
"#;

pub const TEXT_PS_SM3: &str = r#"
sampler2D atlas : register(s0);

float4 main(float2 uv : TEXCOORD0, float4 color : COLOR0) : COLOR
{
    float coverage = tex2D(atlas, uv).r;
    return float4(color.rgb, color.a * coverage);
}
"#;

pub fn compile(source: &str, entry: PCSTR, target: PCSTR) -> Result<ID3DBlob, String> {
    let mut code = None;
    let mut errors = None;
    let compiled = unsafe {
        D3DCompile(
            source.as_ptr() as *const _,
            source.len(),
            PCSTR::null(),
            None,
            Option::<&ID3DInclude>::None,
            entry,
            target,
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };

    if let Err(err) = compiled {
        if let Some(errors) = errors {
            return Err(blob_text(&errors));
        }

        return Err(err.to_string());
    }

    code.ok_or_else(|| "shader".to_string())
}

pub fn vs5(source: &str, entry: PCSTR) -> Result<ID3DBlob, String> {
    compile(source, entry, s!("vs_5_0"))
}

pub fn ps5(source: &str, entry: PCSTR) -> Result<ID3DBlob, String> {
    compile(source, entry, s!("ps_5_0"))
}

pub fn vs3(source: &str) -> Result<ID3DBlob, String> {
    compile(source, s!("main"), s!("vs_3_0"))
}

pub fn ps3(source: &str) -> Result<ID3DBlob, String> {
    compile(source, s!("main"), s!("ps_3_0"))
}

pub fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())
    }
}

pub fn blob_text(blob: &ID3DBlob) -> String {
    String::from_utf8_lossy(blob_bytes(blob)).trim().to_string()
}
