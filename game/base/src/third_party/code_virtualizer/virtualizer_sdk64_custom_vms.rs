// ******************************************************************************
// Header: VirtualizerSDK64_CustomVMs.rs
// Description: Rust macros definitions
//
// Author/s: Oreans Technologies
// (c) 2024 Oreans Technologies
//
// --- File generated automatically from Oreans VM Generator (21/7/2024) ---
// ******************************************************************************

#[allow(dead_code)]
#[cfg_attr(
    all(
        target_arch = "aarch64",
        any(target_os = "linux", target_os = "macos")
    ),
    link(name = "VirtualizerARM64SDK", kind = "dylib")
)]
#[cfg_attr(
    all(target_arch = "aarch64", windows),
    link(name = "VirtualizerArm64SDK", kind = "dylib")
)]
#[cfg_attr(
    all(
        not(target_arch = "aarch64"),
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ),
    link(name = "VirtualizerSDK64", kind = "dylib")
)]
extern "C" {
    #[link_name = "CustomVM00000103_Start"]
    fn VIRTUALIZER_TIGER_WHITE_START();

    #[link_name = "CustomVM00000103_End"]
    fn VIRTUALIZER_TIGER_WHITE_END();

    #[link_name = "CustomVM00000104_Start"]
    fn VIRTUALIZER_TIGER_RED_START();

    #[link_name = "CustomVM00000104_End"]
    fn VIRTUALIZER_TIGER_RED_END();

    #[link_name = "CustomVM00000105_Start"]
    fn VIRTUALIZER_TIGER_BLACK_START();

    #[link_name = "CustomVM00000105_End"]
    fn VIRTUALIZER_TIGER_BLACK_END();

    #[link_name = "CustomVM00000107_Start"]
    fn VIRTUALIZER_FISH_WHITE_START();

    #[link_name = "CustomVM00000107_End"]
    fn VIRTUALIZER_FISH_WHITE_END();

    #[link_name = "CustomVM00000109_Start"]
    fn VIRTUALIZER_FISH_RED_START();

    #[link_name = "CustomVM00000109_End"]
    fn VIRTUALIZER_FISH_RED_END();

    #[link_name = "CustomVM00000111_Start"]
    fn VIRTUALIZER_FISH_BLACK_START();

    #[link_name = "CustomVM00000111_End"]
    fn VIRTUALIZER_FISH_BLACK_END();

    #[link_name = "CustomVM00000113_Start"]
    fn VIRTUALIZER_PUMA_WHITE_START();

    #[link_name = "CustomVM00000113_End"]
    fn VIRTUALIZER_PUMA_WHITE_END();

    #[link_name = "CustomVM00000115_Start"]
    fn VIRTUALIZER_PUMA_RED_START();

    #[link_name = "CustomVM00000115_End"]
    fn VIRTUALIZER_PUMA_RED_END();

    #[link_name = "CustomVM00000117_Start"]
    fn VIRTUALIZER_PUMA_BLACK_START();

    #[link_name = "CustomVM00000117_End"]
    fn VIRTUALIZER_PUMA_BLACK_END();

    #[link_name = "CustomVM00000119_Start"]
    fn VIRTUALIZER_SHARK_WHITE_START();

    #[link_name = "CustomVM00000119_End"]
    fn VIRTUALIZER_SHARK_WHITE_END();

    #[link_name = "CustomVM00000121_Start"]
    fn VIRTUALIZER_SHARK_RED_START();

    #[link_name = "CustomVM00000121_End"]
    fn VIRTUALIZER_SHARK_RED_END();

    #[link_name = "CustomVM00000123_Start"]
    fn VIRTUALIZER_SHARK_BLACK_START();

    #[link_name = "CustomVM00000123_End"]
    fn VIRTUALIZER_SHARK_BLACK_END();

    #[link_name = "CustomVM00000135_Start"]
    fn VIRTUALIZER_DOLPHIN_WHITE_START();

    #[link_name = "CustomVM00000135_End"]
    fn VIRTUALIZER_DOLPHIN_WHITE_END();

    #[link_name = "CustomVM00000137_Start"]
    fn VIRTUALIZER_DOLPHIN_RED_START();

    #[link_name = "CustomVM00000137_End"]
    fn VIRTUALIZER_DOLPHIN_RED_END();

    #[link_name = "CustomVM00000139_Start"]
    fn VIRTUALIZER_DOLPHIN_BLACK_START();

    #[link_name = "CustomVM00000139_End"]
    fn VIRTUALIZER_DOLPHIN_BLACK_END();

    #[link_name = "CustomVM00000147_Start"]
    fn VIRTUALIZER_EAGLE_WHITE_START();

    #[link_name = "CustomVM00000147_End"]
    fn VIRTUALIZER_EAGLE_WHITE_END();

    #[link_name = "CustomVM00000149_Start"]
    fn VIRTUALIZER_EAGLE_RED_START();

    #[link_name = "CustomVM00000149_End"]
    fn VIRTUALIZER_EAGLE_RED_END();

    #[link_name = "CustomVM00000151_Start"]
    fn VIRTUALIZER_EAGLE_BLACK_START();

    #[link_name = "CustomVM00000151_End"]
    fn VIRTUALIZER_EAGLE_BLACK_END();

    #[link_name = "CustomVM00000161_Start"]
    fn VIRTUALIZER_LION_WHITE_START();

    #[link_name = "CustomVM00000161_End"]
    fn VIRTUALIZER_LION_WHITE_END();

    #[link_name = "CustomVM00000163_Start"]
    fn VIRTUALIZER_LION_RED_START();

    #[link_name = "CustomVM00000163_End"]
    fn VIRTUALIZER_LION_RED_END();

    #[link_name = "CustomVM00000165_Start"]
    fn VIRTUALIZER_LION_BLACK_START();

    #[link_name = "CustomVM00000165_End"]
    fn VIRTUALIZER_LION_BLACK_END();

    #[link_name = "CustomVM00000167_Start"]
    fn VIRTUALIZER_COBRA_WHITE_START();

    #[link_name = "CustomVM00000167_End"]
    fn VIRTUALIZER_COBRA_WHITE_END();

    #[link_name = "CustomVM00000169_Start"]
    fn VIRTUALIZER_COBRA_RED_START();

    #[link_name = "CustomVM00000169_End"]
    fn VIRTUALIZER_COBRA_RED_END();

    #[link_name = "CustomVM00000171_Start"]
    fn VIRTUALIZER_COBRA_BLACK_START();

    #[link_name = "CustomVM00000171_End"]
    fn VIRTUALIZER_COBRA_BLACK_END();

    #[link_name = "CustomVM00000173_Start"]
    fn VIRTUALIZER_WOLF_WHITE_START();

    #[link_name = "CustomVM00000173_End"]
    fn VIRTUALIZER_WOLF_WHITE_END();

    #[link_name = "CustomVM00000175_Start"]
    fn VIRTUALIZER_WOLF_RED_START();

    #[link_name = "CustomVM00000175_End"]
    fn VIRTUALIZER_WOLF_RED_END();

    #[link_name = "CustomVM00000177_Start"]
    fn VIRTUALIZER_WOLF_BLACK_START();

    #[link_name = "CustomVM00000177_End"]
    fn VIRTUALIZER_WOLF_BLACK_END();

    #[link_name = "Mutate_Start"]
    fn VIRTUALIZER_MUTATE_ONLY_START();

    #[link_name = "Mutate_End"]
    fn VIRTUALIZER_MUTATE_ONLY_END();

    #[link_name = "CustomVM00000218_Start"]
    fn VIRTUALIZER_FALCON_TINY_START();

    #[link_name = "CustomVM00000218_End"]
    fn VIRTUALIZER_FALCON_TINY_END();

}
