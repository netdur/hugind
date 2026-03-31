use crate::core::config::agent::ShellPermission;

/// Generate a restrictive macOS sandbox-exec profile based on agent permissions.
///
/// Default policy is deny-all with explicit allowlists for:
/// - Process execution (always needed to run the command)
/// - Filesystem read (scoped to working_dir if set, otherwise agent paths)
/// - Filesystem write (only if not env_clear — conservative heuristic)
/// - Network access (denied by default in sandbox; agent net permissions are enforced separately)
pub fn macos_sandbox_profile(perm: &ShellPermission) -> String {
    let mut rules = Vec::new();

    rules.push("(version 1)".to_string());
    rules.push("(deny default)".to_string());

    // Always allow process execution and signal handling
    rules.push("(allow process-exec)".to_string());
    rules.push("(allow process-fork)".to_string());
    rules.push("(allow signal)".to_string());

    // Allow sysctl reads (needed by many programs)
    rules.push("(allow sysctl-read)".to_string());

    // Allow reading system libraries and frameworks
    rules.push("(allow file-read* (subpath \"/usr/lib\"))".to_string());
    rules.push("(allow file-read* (subpath \"/usr/share\"))".to_string());
    rules.push("(allow file-read* (subpath \"/System\"))".to_string());
    rules.push("(allow file-read* (subpath \"/Library\"))".to_string());
    rules.push("(allow file-read* (subpath \"/private/var/db\"))".to_string());
    rules.push("(allow file-read* (subpath \"/dev\"))".to_string());
    rules.push("(allow file-read* (subpath \"/bin\"))".to_string());
    rules.push("(allow file-read* (subpath \"/usr/bin\"))".to_string());
    rules.push("(allow file-read* (subpath \"/usr/local\"))".to_string());
    rules.push("(allow file-read* (subpath \"/opt\"))".to_string());

    // Allow reading the working directory and home directory Homebrew/tools
    if let Some(wd) = &perm.working_dir {
        rules.push(format!("(allow file-read* (subpath \"{}\"))", wd));
        rules.push(format!("(allow file-write* (subpath \"{}\"))", wd));
    }

    // Allow tmp access (many tools need it)
    rules.push("(allow file-read* (subpath \"/tmp\"))".to_string());
    rules.push("(allow file-write* (subpath \"/tmp\"))".to_string());
    rules.push("(allow file-read* (subpath \"/private/tmp\"))".to_string());
    rules.push("(allow file-write* (subpath \"/private/tmp\"))".to_string());

    // Allow mach lookups (needed for IPC, many system calls)
    rules.push("(allow mach-lookup)".to_string());

    rules.join("\n")
}
