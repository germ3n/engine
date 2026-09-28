fn main() {
    cc::Build::new()
        .file("src/ui/vr_openvr.c")
        .compile("vr_openvr");
}
