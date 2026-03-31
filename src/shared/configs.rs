const RESERVED_CONFIG_NAMES: &[&str] = &["config"];

pub fn is_reserved_config_name(name: &str) -> bool {
    RESERVED_CONFIG_NAMES.contains(&name)
}
