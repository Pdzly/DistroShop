use base64::{encoded_len, engine::general_purpose::STANDARD, Engine as _};
use std::sync::LazyLock;

fn data_url(bytes: &[u8], mime_type: &str) -> String {
    let prefix = "data:".len() + mime_type.len() + ";base64,".len();
    let payload = encoded_len(bytes.len(), true).expect("embedded logo is too large to encode");
    let mut source = String::with_capacity(prefix + payload);
    source.push_str("data:");
    source.push_str(mime_type);
    source.push_str(";base64,");
    STANDARD.encode_string(bytes, &mut source);
    source
}

static ARCH_LINUX: LazyLock<String> = LazyLock::new(|| {
    data_url(include_bytes!("../assets/logos/arch-linux.svg"), "image/svg+xml")
});
static TINY_CORE_LINUX: LazyLock<String> = LazyLock::new(|| {
    data_url(include_bytes!("../assets/logos/tiny-core-linux.png"), "image/png")
});
static LINUX_MINT: LazyLock<String> = LazyLock::new(|| {
    data_url(include_bytes!("../assets/logos/linux-mint.svg"), "image/svg+xml")
});
static UBUNTU: LazyLock<String> =
    LazyLock::new(|| data_url(include_bytes!("../assets/logos/ubuntu.svg"), "image/svg+xml"));
static DEBIAN: LazyLock<String> =
    LazyLock::new(|| data_url(include_bytes!("../assets/logos/debian.svg"), "image/svg+xml"));
static FEDORA: LazyLock<String> =
    LazyLock::new(|| data_url(include_bytes!("../assets/logos/fedora.svg"), "image/svg+xml"));
static MANJARO: LazyLock<String> =
    LazyLock::new(|| data_url(include_bytes!("../assets/logos/manjaro.svg"), "image/svg+xml"));
static OPENSUSE_TUMBLEWEED: LazyLock<String> = LazyLock::new(|| {
    data_url(
        include_bytes!("../assets/logos/opensuse-tumbleweed.svg"),
        "image/svg+xml",
    )
});
static KALI_LINUX: LazyLock<String> = LazyLock::new(|| {
    data_url(include_bytes!("../assets/logos/kali-linux.svg"), "image/svg+xml")
});
static ALPINE_LINUX: LazyLock<String> = LazyLock::new(|| {
    data_url(include_bytes!("../assets/logos/alpine-linux.svg"), "image/svg+xml")
});
static GENTOO: LazyLock<String> =
    LazyLock::new(|| data_url(include_bytes!("../assets/logos/gentoo.svg"), "image/svg+xml"));
static NIXOS: LazyLock<String> =
    LazyLock::new(|| data_url(include_bytes!("../assets/logos/nixos.svg"), "image/svg+xml"));

pub(crate) fn distro_logo_source<'a>(name: &str, fallback: &'a str) -> &'a str {
    match name {
        "arch linux" => ARCH_LINUX.as_str(),
        "tiny core linux" => TINY_CORE_LINUX.as_str(),
        "linux mint" => LINUX_MINT.as_str(),
        "ubuntu" => UBUNTU.as_str(),
        "debian" => DEBIAN.as_str(),
        "fedora workstation" => FEDORA.as_str(),
        "manjaro" => MANJARO.as_str(),
        "opensuse tumbleweed" => OPENSUSE_TUMBLEWEED.as_str(),
        "kali linux" => KALI_LINUX.as_str(),
        "alpine linux" => ALPINE_LINUX.as_str(),
        "gentoo" => GENTOO.as_str(),
        "nixos" => NIXOS.as_str(),
        _ => fallback,
    }
}
