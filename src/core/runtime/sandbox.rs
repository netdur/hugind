use crate::core::config::agent::ShellPermission;

/// Generate a macOS sandbox-exec profile based on agent permissions.
///
/// Policy follows what the YAML declares:
/// - If shell.allow is true with no whitelist → permissive (allow default)
/// - If shell has a whitelist or blacklist → restrictive (deny default + explicit allows)
///
/// The YAML permission model is the source of truth. The sandbox enforces
/// filesystem scoping when a working_dir is set, but doesn't restrict
/// program execution beyond what the whitelist/blacklist declares.
pub fn macos_sandbox_profile(perm: &ShellPermission) -> String {
    let has_restrictions = perm.whitelist.is_some() || perm.blacklist.is_some();

    if !has_restrictions {
        // No whitelist/blacklist — permissive sandbox.
        // Program-level restrictions are enforced by Hugind's own
        // ensure_program_allowed() check, not by the OS sandbox.
        return "(version 1) (allow default)".to_string();
    }

    // Restrictive mode — only when the YAML declares specific constraints
    let mut rules = Vec::new();

    rules.push("(version 1)".to_string());
    rules.push("(deny default)".to_string());

    // Process execution and signals
    rules.push("(allow process-exec)".to_string());
    rules.push("(allow process-fork)".to_string());
    rules.push("(allow signal)".to_string());
    rules.push("(allow sysctl-read)".to_string());
    rules.push("(allow mach-lookup)".to_string());

    // System libraries and tools
    rules.push("(allow file-read* (subpath \"/usr\"))".to_string());
    rules.push("(allow file-read* (subpath \"/System\"))".to_string());
    rules.push("(allow file-read* (subpath \"/Library\"))".to_string());
    rules.push("(allow file-read* (subpath \"/bin\"))".to_string());
    rules.push("(allow file-read* (subpath \"/opt\"))".to_string());
    rules.push("(allow file-read* (subpath \"/dev\"))".to_string());
    rules.push("(allow file-read* (subpath \"/private/var/db\"))".to_string());
    rules.push("(allow file-read* (subpath \"/private/etc\"))".to_string());

    // Temp
    rules.push("(allow file-read* (subpath \"/tmp\"))".to_string());
    rules.push("(allow file-write* (subpath \"/tmp\"))".to_string());
    rules.push("(allow file-read* (subpath \"/private/tmp\"))".to_string());
    rules.push("(allow file-write* (subpath \"/private/tmp\"))".to_string());

    // Working directory access
    if let Some(wd) = &perm.working_dir {
        rules.push(format!("(allow file-read* (subpath \"{}\"))", wd));
        rules.push(format!("(allow file-write* (subpath \"{}\"))", wd));
    }

    // Home directory read access (for tools, configs, etc.)
    if let Some(home) = dirs::home_dir() {
        rules.push(format!(
            "(allow file-read* (subpath \"{}\"))",
            home.display()
        ));
    }

    rules.join("\n")
}
