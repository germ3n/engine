fn main() {
    write_wiki();
    stage_steam();
    link_virtualizer();
    compile_bundled_lua();
    write_base_pak();
    compile_sound_device();

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();

    if target_os == "android" || target_os == "ios" {
        cc::Build::new().file("src/ui/vr_stub.c").compile("vr_stub");

        if target_os == "android" {
            println!("cargo:rustc-link-lib=log");
            println!("cargo:rustc-link-lib=android");
            println!("cargo:rustc-link-lib=EGL");
            println!("cargo:rustc-link-lib=GLESv3");
        }

        if target_os == "ios" {
            cc::Build::new()
                .file("src/ui/metal_ios.m")
                .compile("metal_ios");
            println!("cargo:rustc-link-lib=framework=UIKit");
            println!("cargo:rustc-link-lib=framework=QuartzCore");
            println!("cargo:rustc-link-lib=framework=Foundation");
            println!("cargo:rustc-link-lib=framework=CoreGraphics");
            println!("cargo:rustc-link-lib=framework=Metal");
        }

        return;
    }

    cc::Build::new()
        .file("src/ui/vr_openvr.c")
        .compile("vr_openvr");
    let pointer_width = std::env::var("CARGO_CFG_TARGET_POINTER_WIDTH").unwrap();
    let mut openvr = cc::Build::new();
    openvr
        .cpp(true)
        .std("c++17")
        .warnings(false)
        .include("third_party/openvr/headers")
        .include("third_party/openvr/src")
        .include("third_party/openvr/src/vrcore")
        .define("VRCORE_NO_PLATFORM", None)
        .define("OPENVR_BUILD_STATIC", None)
        .define("VR_API_PUBLIC", None)
        .file("third_party/openvr/src/openvr_api_public.cpp")
        .file("third_party/openvr/src/jsoncpp.cpp")
        .file("third_party/openvr/src/vrcore/dirtools_public.cpp")
        .file("third_party/openvr/src/vrcore/envvartools_public.cpp")
        .file("third_party/openvr/src/vrcore/hmderrors_public.cpp")
        .file("third_party/openvr/src/vrcore/sharedlibtools_public.cpp")
        .file("third_party/openvr/src/vrcore/strtools_public.cpp")
        .file("third_party/openvr/src/vrcore/pathtools_public.cpp")
        .file("third_party/openvr/src/vrcore/vrpathregistry_public.cpp");

    if target_os == "macos" {
        openvr.define("OSX", None).define("POSIX", None);
        openvr.flag("-x").flag("objective-c++");
        println!("cargo:rustc-link-lib=framework=Foundation");
    }

    if target_os == "linux" {
        openvr.define("LINUX", None).define("POSIX", None);
        println!("cargo:rustc-link-lib=dylib=dl");

        if std::env::var("CARGO_CFG_TARGET_ARCH").unwrap() == "aarch64" {
            openvr.define("LINUXARM64", None);
        } else if pointer_width == "64" {
            openvr.define("LINUX64", None);
        }
    }

    if target_os == "windows" {
        openvr.define("WIN32", None);

        if pointer_width == "64" {
            openvr.define("WIN64", None);
        }

        println!("cargo:rustc-link-lib=shell32");
    }

    openvr.compile("openvr_api");
}

fn stage_steam() {
    if std::env::var("CARGO_FEATURE_STEAM").is_err() {
        return;
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let pointer =
        std::env::var("CARGO_CFG_TARGET_POINTER_WIDTH").unwrap_or_else(|_| "64".to_string());
    let name = match target_os.as_str() {
        "windows" if pointer == "64" => "steam_api64.dll",
        "windows" => "steam_api.dll",
        "linux" => "libsteam_api.so",
        "macos" => "libsteam_api.dylib",
        _ => return,
    };
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let Some(build_dir) = out_dir.parent().and_then(|path| path.parent()) else {
        return;
    };

    let mut source = None;
    if let Ok(entries) = std::fs::read_dir(build_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = path
                .file_name()
                .and_then(|text| text.to_str())
                .unwrap_or("");
            if !file_name.starts_with("steamworks-sys-") {
                continue;
            }

            let candidate = path.join("out").join(name);
            if candidate.exists() {
                source = Some(candidate);

                break;
            }
        }
    }

    let Some(source) = source else {
        println!("cargo:warning=steam library {name} was not found");

        return;
    };

    let Some(dest_dir) = build_dir.parent() else {
        return;
    };

    let dest = dest_dir.join(name);
    if let Err(err) = std::fs::copy(&source, &dest) {
        println!("cargo:warning=failed to copy {name}: {err}");

        return;
    }

    if target_os == "macos" {
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path");
    }

    if target_os == "linux" {
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
    }
}

fn link_virtualizer()
{
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let lib_dir = manifest_dir.join("src/third_party/code_virtualizer/lib");
    println!("cargo:rerun-if-changed={}", lib_dir.display());
    println!("cargo:rustc-link-search=native={}", lib_dir.display());

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let file_name = match (target_os.as_str(), target_arch.as_str())
    {
        ("macos", "aarch64") => "libVirtualizerARM64SDK.dylib",
        ("linux", "aarch64") => "libVirtualizerARM64SDK.so",
        ("linux", _) => "libVirtualizerSDK64.so",
        ("windows", "aarch64") => "VirtualizerArm64SDK.lib",
        ("windows", _) => "VirtualizerSDK64.lib",
        _ =>
        {

            return;
        }
    };
    let lib_path = lib_dir.join(file_name);

    if !lib_path.exists()
    {
        println!(
            "cargo:warning=virtualizer library {} was not found",
            lib_path.display()
        );

        return;
    }

    if target_os == "macos"
    {
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path");
        println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path");
    }

    if target_os == "linux"
    {
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
    }

    if target_os == "windows"
    {
        let lib_name = match target_arch.as_str()
        {
            "aarch64" => "VirtualizerArm64SDK",
            _ => "VirtualizerSDK64",
        };

        println!("cargo:rustc-link-lib={lib_name}");
    }

    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let Some(dest_dir) = out_dir
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
    else
    {

        return;
    };
    let dest = dest_dir.join(file_name);

    if let Err(err) = std::fs::copy(&lib_path, &dest)
    {
        println!("cargo:warning=failed to copy {file_name}: {err}");
    }
}

fn compile_sound_device() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    println!("cargo:rerun-if-changed=src/sound/device.c");
    println!("cargo:rerun-if-changed=third_party/miniaudio/miniaudio.h");
    cc::Build::new()
        .file("src/sound/device.c")
        .include("third_party/miniaudio")
        .warnings(false)
        .compile("sound_device");

    match target_os.as_str() {
        "macos" | "ios" => {
            println!("cargo:rustc-link-lib=framework=AudioToolbox");
            println!("cargo:rustc-link-lib=framework=CoreAudio");
            println!("cargo:rustc-link-lib=framework=CoreFoundation");

            if target_os == "ios" {
                println!("cargo:rustc-link-lib=framework=AVFoundation");
            }
        }
        "android" => {
            println!("cargo:rustc-link-lib=dl");
            println!("cargo:rustc-link-lib=m");
        }
        "linux" => {
            println!("cargo:rustc-link-lib=dl");
            println!("cargo:rustc-link-lib=pthread");
            println!("cargo:rustc-link-lib=m");
        }
        "windows" => {
            println!("cargo:rustc-link-lib=ole32");
            println!("cargo:rustc-link-lib=user32");
        }
        _ => {}
    }
}

fn compile_bundled_lua() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let lua_out = out_dir.join("lua");
    let strip = std::env::var("PROFILE").unwrap() == "release";
    let sources = [
        "src/script/libs/hook.lua",
        "src/script/libs/net.lua",
        "src/script/libs/vector3.lua",
        "src/script/libs/angle3.lua",
        "src/script/libs/ents.lua",
        "src/script/libs/scripted_ents.lua",
        "src/script/libs/sound.lua",
        "src/lua/menu/menu.lua",
    ];

    for source in sources {
        let src = manifest_dir.join(source);
        let stem = src.file_stem().unwrap().to_string_lossy();
        let dest = lua_out.join(format!("{stem}.luac"));
        compile_lua(&src, &dest, strip);
    }

    for relative in content_scripts(&manifest_dir) {
        let src = manifest_dir.join("src/lua").join(&relative);
        let dest = lua_out.join("content").join(format!("{relative}c"));
        compile_lua(&src, &dest, strip);
    }
}

const CONTENT_DIRS: [&str; 2] = ["autorun", "entities"];

fn content_scripts(manifest_dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();

    for dir in CONTENT_DIRS {
        let root = manifest_dir.join("src/lua").join(dir);
        println!("cargo:rerun-if-changed={}", root.display());
        collect_lua(&root, dir, &mut out);
    }

    out.sort();

    out
}

fn collect_lua(dir: &std::path::Path, relative: &str, out: &mut Vec<String>) {
    let Ok(listing) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in listing.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let child = format!("{relative}/{name}");

        if path.is_dir() {
            collect_lua(&path, &child, out);
        } else if name.ends_with(".lua") {
            out.push(child);
        }
    }
}

fn write_wiki() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest_dir.join("src");
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("src/script/libs/engine.rs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("src/script/libs/net.rs").display()
    );
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let dest = wiki_html(&out_dir, &manifest_dir);

    if let Err(err) = wiki::generate(&src, &dest) {
        panic!("wiki: {err}");
    }
}

fn wiki_html(out_dir: &std::path::Path, manifest_dir: &std::path::Path) -> std::path::PathBuf {
    let target_dir = out_dir
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .map(|path| path.to_path_buf())
        .unwrap_or_else(|| manifest_dir.join("../../target"));

    target_dir.join("wiki/index.html")
}

fn write_base_pak() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let lua_out = out_dir.join("lua");
    let shaders = [
        ("shaders/mesh.wgsl", "src/ui/shaders/mesh.wgsl"),
        ("shaders/color.wgsl", "src/ui/shaders/color.wgsl"),
        ("shaders/text.wgsl", "src/ui/shaders/text.wgsl"),
        ("shaders/skinned.wgsl", "src/ui/shaders/skinned.wgsl"),
        ("models/test.mdl", "models/test.mdl"),
        ("models/test.anm", "models/test.anm"),
        ("models/test.gltf", "models/test.gltf"),
        ("models/test.bin", "models/test.bin"),
        ("models/test.png", "models/test.png"),
        ("sound/mannequin/wave.wav", "sound/mannequin/wave.wav"),
    ];
    let lua = [
        ("lua/libs/hook.luac", "hook.luac"),
        ("lua/libs/net.luac", "net.luac"),
        ("lua/libs/vector3.luac", "vector3.luac"),
        ("lua/libs/angle3.luac", "angle3.luac"),
        ("lua/libs/ents.luac", "ents.luac"),
        ("lua/libs/scripted_ents.luac", "scripted_ents.luac"),
        ("lua/libs/sound.luac", "sound.luac"),
        ("lua/menu/menu.luac", "menu.luac"),
    ];
    let mut owned = Vec::new();
    let mut files = Vec::new();

    for (virtual_path, file_name) in lua {
        let path = lua_out.join(file_name);
        let bytes = std::fs::read(&path).unwrap_or_else(|err| {
            panic!("failed to read {}: {err}", path.display());
        });
        owned.push((virtual_path.to_string(), bytes));
    }

    for relative in content_scripts(&manifest_dir) {
        let path = lua_out.join("content").join(format!("{relative}c"));
        let bytes = std::fs::read(&path).unwrap_or_else(|err| {
            panic!("failed to read {}: {err}", path.display());
        });
        owned.push((format!("lua/{relative}c"), bytes));
    }

    for (virtual_path, source) in shaders {
        let path = manifest_dir.join(source);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = std::fs::read(&path).unwrap_or_else(|err| {
            panic!("failed to read {}: {err}", path.display());
        });
        owned.push((virtual_path.to_string(), bytes));
    }

    let wiki_path = wiki_html(&out_dir, &manifest_dir);
    let wiki_bytes = std::fs::read(&wiki_path).unwrap_or_else(|err| {
        panic!("failed to read {}: {err}", wiki_path.display());
    });
    owned.push(("wiki/index.html".to_string(), wiki_bytes));

    for (name, bytes) in &owned {
        files.push((name.as_str(), bytes.as_slice()));
    }

    let bytes = encode_pak(&files).unwrap_or_else(|err| panic!("failed to encode base.pak: {err}"));
    let out_pak = out_dir.join("base.pak");
    std::fs::write(&out_pak, &bytes).unwrap_or_else(|err| {
        panic!("failed to write {}: {err}", out_pak.display());
    });

    let manifest_pak = manifest_dir.join("base.pak");
    let _ = std::fs::write(&manifest_pak, &bytes);

    if let Some(build_dir) = out_dir.parent().and_then(|path| path.parent()) {
        if let Some(target_dir) = build_dir.parent() {
            let target_pak = target_dir.join("base.pak");
            let _ = std::fs::write(&target_pak, &bytes);
            let game_dir = target_dir.join("game");
            let _ = std::fs::create_dir_all(&game_dir);
            let _ = std::fs::write(game_dir.join("base.pak"), &bytes);
        }
    }
}

fn compile_lua(src: &std::path::Path, dest: &std::path::Path, strip: bool) {
    println!("cargo:rerun-if-changed={}", src.display());

    let source = std::fs::read(src).unwrap_or_else(|err| {
        panic!("failed to read {}: {err}", src.display());
    });
    let lua = mlua::Lua::new();
    let chunk_name = src
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("chunk.lua");
    let function = lua
        .load(&source)
        .set_name(chunk_name)
        .into_function()
        .unwrap_or_else(|err| {
            panic!("failed to compile {}: {err}", src.display());
        });
    let string_lib: mlua::Table = lua.globals().get("string").expect("string library missing");
    let dump: mlua::Function = string_lib.get("dump").expect("string.dump missing");
    let bytecode: mlua::LuaString = dump.call((function, strip)).unwrap_or_else(|err| {
        panic!("failed to dump {}: {err}", src.display());
    });
    let bytes = bytecode.as_bytes();

    if !bytes.starts_with(b"\x1bLJ") {
        panic!("compiled {} is not LuaJIT bytecode", src.display());
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|err| {
            panic!("failed to create {}: {err}", parent.display());
        });
    }

    std::fs::write(dest, bytes.as_ref()).unwrap_or_else(|err| {
        panic!("failed to write {}: {err}", dest.display());
    });
}

#[derive(wincode::SchemaWrite, wincode::SchemaRead)]
struct PakEntry {
    name: String,
    offset: u64,
    compressed: u64,
    raw_size: u64,
}

#[derive(wincode::SchemaWrite, wincode::SchemaRead)]
struct PakCatalog {
    version: u32,
    entries: Vec<PakEntry>,
}

fn encode_pak(files: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    const MAGIC: &[u8; 4] = b"PAK\0";
    const VERSION: u32 = 1;
    let mut frames = Vec::with_capacity(files.len());
    let mut entries = Vec::with_capacity(files.len());

    for (name, bytes) in files {
        let frame = zstd::bulk::compress(bytes, zstd::DEFAULT_COMPRESSION_LEVEL)
            .map_err(|err| format!("pak: {err}"))?;
        entries.push(PakEntry {
            name: (*name).to_string(),
            offset: 0,
            compressed: frame.len() as u64,
            raw_size: bytes.len() as u64,
        });
        frames.push(frame);
    }

    let mut catalog = PakCatalog {
        version: VERSION,
        entries,
    };
    let toc_len = wincode::serialized_size(&catalog).map_err(|err| format!("pak: {err}"))?;
    let mut cursor = (4 + toc_len + 7) & !7;
    let mut idx = 0;

    while idx < catalog.entries.len() {
        catalog.entries[idx].offset = cursor;
        cursor = (cursor + catalog.entries[idx].compressed + 7) & !7;
        idx += 1;
    }

    let toc = wincode::serialize(&catalog).map_err(|err| format!("pak: {err}"))?;
    let mut out = Vec::with_capacity(cursor as usize);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&toc);
    idx = 0;

    while idx < frames.len() {
        while (out.len() as u64) < catalog.entries[idx].offset {
            out.push(0);
        }

        out.extend_from_slice(&frames[idx]);
        idx += 1;
    }

    Ok(out)
}
