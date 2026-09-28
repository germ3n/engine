fn main() {
    compile_bundled_lua();

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();

    if target_os == "android" || target_os == "ios" {
        cc::Build::new()
            .file("src/ui/vr_stub.c")
            .compile("vr_stub");

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

fn compile_bundled_lua()
{
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let lua_out = out_dir.join("lua");
    let strip = std::env::var("PROFILE").unwrap() == "release";
    let sources = [
        "src/script/libs/hook.lua",
        "src/script/libs/net.lua",
        "src/script/libs/vector3.lua",
        "src/script/libs/angle3.lua",
        "src/lua/menu/menu.lua",
    ];

    for source in sources
    {
        let src = manifest_dir.join(source);
        let stem = src.file_stem().unwrap().to_string_lossy();
        let dest = lua_out.join(format!("{stem}.luac"));
        compile_lua(&src, &dest, strip);
    }
}

fn compile_lua(src: &std::path::Path, dest: &std::path::Path, strip: bool)
{
    println!("cargo:rerun-if-changed={}", src.display());

    let source = std::fs::read(src).unwrap_or_else(|err| {
        panic!("failed to read {}: {err}", src.display());
    });
    let lua = mlua::Lua::new();
    let chunk_name = src.file_name().and_then(|name| name.to_str()).unwrap_or("chunk.lua");
    let function = lua.load(&source).set_name(chunk_name).into_function().unwrap_or_else(|err| {
        panic!("failed to compile {}: {err}", src.display());
    });
    let string_lib: mlua::Table = lua.globals().get("string").expect("string library missing");
    let dump: mlua::Function = string_lib.get("dump").expect("string.dump missing");
    let bytecode: mlua::LuaString = dump.call((function, strip)).unwrap_or_else(|err| {
        panic!("failed to dump {}: {err}", src.display());
    });
    let bytes = bytecode.as_bytes();

    if !bytes.starts_with(b"\x1bLJ")
    {
        panic!("compiled {} is not LuaJIT bytecode", src.display());
    }

    if let Some(parent) = dest.parent()
    {
        std::fs::create_dir_all(parent).unwrap_or_else(|err| {
            panic!("failed to create {}: {err}", parent.display());
        });
    }

    std::fs::write(dest, bytes.as_ref()).unwrap_or_else(|err| {
        panic!("failed to write {}: {err}", dest.display());
    });
}
