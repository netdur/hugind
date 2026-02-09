const RESERVED_CONFIG_NAMES: &[&str] = &[
    "config",
    "cpu_only",
    "cuda_dedicated",
    "metal_unified",
];

pub fn is_reserved_config_name(name: &str) -> bool {
    RESERVED_CONFIG_NAMES.contains(&name)
}
