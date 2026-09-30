use windows::core::{s, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, ID3DInclude};

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

pub fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())
    }
}

pub fn blob_text(blob: &ID3DBlob) -> String {
    String::from_utf8_lossy(blob_bytes(blob)).trim().to_string()
}
