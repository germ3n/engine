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
    #[link_name = "VirtualizerStart"]
    fn VIRTUALIZER_START();

    #[link_name = "VirtualizerEnd"]
    fn VIRTUALIZER_END();

    #[link_name = "VirtualizerStrEncryptStart"]
    fn VIRTUALIZER_STR_ENCRYPT_START();

    #[link_name = "VirtualizerStrEncryptEnd"]
    fn VIRTUALIZER_STR_ENCRYPT_END();

    #[link_name = "VirtualizerStrEncryptWStart"]
    fn VIRTUALIZER_STR_ENCRYPTW_START();

    #[link_name = "VirtualizerStrEncryptWEnd"]
    fn VIRTUALIZER_STR_ENCRYPTW_END();

    #[link_name = "VirtualizerUnprotectedStart"]
    fn VIRTUALIZER_UNPROTECTED_START();

    #[link_name = "VirtualizerUnprotectedEnd"]
    fn VIRTUALIZER_UNPROTECTED_END();
}
