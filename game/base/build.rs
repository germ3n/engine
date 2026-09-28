fn main() {
    cc::Build::new()
        .file("src/ui/vr_openvr.c")
        .compile("vr_openvr");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
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
