use rquickjs::{AsyncContext, Function, Result, function::Async};
use tokio::process::Command;

use crate::core::config::agent::{AgentConfig, ShellPermission};
use crate::core::runtime::util::{parse_duration_string, parse_memory_string};
use crate::shared::logging::RunLogger;

async fn run_process(
    program: String,
    args: Vec<String>,
    perm: ShellPermission,
    logger: Option<RunLogger>,
) -> Result<String> {
    if let Some(l) = &logger {
        l.log_line(format!(
            "host.shell.spawn program={} args={:?}",
            program, args
        ));
    }
    ensure_program_allowed(&program, &perm)
        .map_err(|e| rquickjs::Error::new_loading_message("Shell Error", e))?;

    let mut command = if cfg!(target_os = "macos") {
        let profile = "(version 1) (allow default)";
        let mut cmd = Command::new("sandbox-exec");
        cmd.arg("-p").arg(profile).arg(&program);
        cmd
    } else {
        Command::new(&program)
    };

    if !args.is_empty() {
        command.args(&args);
    }

    if perm.env_clear {
        command.env_clear();
    }
    if let Some(wd) = &perm.working_dir {
        command.current_dir(wd);
    }

    let timeout = perm.timeout.as_deref().and_then(parse_duration_string);

    let output_fut = command.output();
    let output_res = if let Some(t) = timeout {
        match tokio::time::timeout(t, output_fut).await {
            Ok(res) => res,
            Err(_) => {
                return Err(rquickjs::Error::new_loading_message(
                    "Shell Error",
                    "Shell command timed out",
                ));
            }
        }
    } else {
        output_fut.await
    };

    let output = output_res.map_err(|e| {
        rquickjs::Error::new_loading_message(
            "Shell Error",
            format!("Failed to execute command: {}", e),
        )
    })?;

    let max_len = perm
        .max_output
        .as_deref()
        .and_then(parse_memory_string)
        .unwrap_or(1024 * 1024);

    let result_str = format_process_output(
        output.status.success(),
        &output.stdout,
        &output.stderr,
        max_len,
    );

    Ok(result_str)
}

async fn run_command_inner(
    cmd_str: String,
    perm: ShellPermission,
    logger: Option<RunLogger>,
) -> Result<String> {
    let (program, args) = split_command_parts(&cmd_str)
        .map_err(|e| rquickjs::Error::new_loading_message("Shell Error", e))?;

    run_process(program, args, perm, logger).await
}

async fn spawn_inner(
    program: String,
    args: Vec<String>,
    perm: ShellPermission,
    logger: Option<RunLogger>,
) -> Result<String> {
    run_process(program, args, perm, logger).await
}

fn ensure_program_allowed(
    program: &str,
    perm: &ShellPermission,
) -> std::result::Result<(), String> {
    if !perm.allow {
        return Err("Shell execution is disabled.".to_string());
    }

    if let Some(whitelist) = &perm.whitelist {
        if !whitelist.iter().any(|cmd| cmd == program) {
            return Err(format!("Command '{}' is not whitelisted.", program));
        }
    }

    if let Some(blacklist) = &perm.blacklist {
        if blacklist.iter().any(|cmd| cmd == program) {
            return Err(format!("Command '{}' is blacklisted.", program));
        }
    }

    Ok(())
}

fn split_command_parts(cmd_str: &str) -> std::result::Result<(String, Vec<String>), String> {
    let parts: Vec<&str> = cmd_str.split_whitespace().collect();
    if parts.is_empty() {
        return Err("Empty command".to_string());
    }
    let program = parts[0].to_string();
    let args = parts[1..].iter().map(|s| s.to_string()).collect();
    Ok((program, args))
}

fn format_process_output(success: bool, stdout: &[u8], stderr: &[u8], max_len: usize) -> String {
    if success {
        if stdout.len() > max_len {
            let mut s = String::from_utf8_lossy(&stdout[..max_len]).to_string();
            s.push_str("...[truncated]");
            s
        } else {
            String::from_utf8_lossy(stdout).to_string()
        }
    } else {
        let mut s = format!("Error: {}", String::from_utf8_lossy(stderr));
        if s.len() > max_len {
            let mut actual_len = max_len;
            while !s.is_char_boundary(actual_len) {
                actual_len -= 1;
            }
            s.truncate(actual_len);
            s.push_str("...[truncated]");
        }
        s
    }
}

pub async fn install(
    ctx: &AsyncContext,
    config: &AgentConfig,
    logger: Option<RunLogger>,
) -> Result<()> {
    let perm = if let Some(p) = &config.permissions {
        p.shell.clone().unwrap_or_default()
    } else {
        ShellPermission::default()
    };

    ctx.async_with(|ctx| {
        let perm_for_snake = perm.clone();
        let perm_for_camel = perm.clone();
        let logger_snake = logger.clone();
        let logger_camel = logger.clone();
        Box::pin(async move {
            let run_command_fn = Function::new(
                ctx.clone(),
                Async(move |cmd: String| {
                    let perm = perm_for_snake.clone();
                    let logger = logger_snake.clone();
                    async move { run_command_inner(cmd, perm, logger).await }
                }),
            )?;

            let run_command_fn_camel = Function::new(
                ctx.clone(),
                Async(move |cmd: String| {
                    let perm = perm_for_camel.clone();
                    let logger = logger_camel.clone();
                    async move { run_command_inner(cmd, perm, logger).await }
                }),
            )?;

            let spawn_fn = Function::new(
                ctx.clone(),
                Async(move |program: String, args: Vec<String>| {
                    let perm = perm.clone();
                    let logger = logger.clone();
                    async move { spawn_inner(program, args, perm, logger).await }
                }),
            )?;

            ctx.globals().set("run_command", run_command_fn)?;
            ctx.globals().set("runCommand", run_command_fn_camel)?;
            ctx.globals().set("spawn", spawn_fn)?;
            Ok(())
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::{ensure_program_allowed, format_process_output, run_process, split_command_parts};
    use crate::core::config::agent::ShellPermission;
    #[cfg(not(target_os = "macos"))]
    use tempfile::tempdir;

    #[test]
    fn rejects_when_shell_is_disabled() {
        let perm = ShellPermission::default();
        let err = ensure_program_allowed("echo", &perm).expect_err("must reject");
        assert!(err.contains("Shell execution is disabled."));
    }

    #[test]
    fn enforces_whitelist_when_present() {
        let mut perm = ShellPermission::default();
        perm.allow = true;
        perm.whitelist = Some(vec!["echo".to_string()]);
        assert!(ensure_program_allowed("echo", &perm).is_ok());
        let err = ensure_program_allowed("ls", &perm).expect_err("must reject");
        assert!(err.contains("not whitelisted"));
    }

    #[test]
    fn enforces_blacklist_when_present() {
        let mut perm = ShellPermission::default();
        perm.allow = true;
        perm.blacklist = Some(vec!["rm".to_string()]);
        assert!(ensure_program_allowed("echo", &perm).is_ok());
        let err = ensure_program_allowed("rm", &perm).expect_err("must reject");
        assert!(err.contains("blacklisted"));
    }

    #[test]
    fn parses_command_into_program_and_args() {
        let (program, args) = split_command_parts("git status --short").expect("split");
        assert_eq!(program, "git");
        assert_eq!(args, vec!["status", "--short"]);
    }

    #[test]
    fn rejects_empty_command_string() {
        let err = split_command_parts("   ").expect_err("must reject");
        assert!(err.contains("Empty command"));
    }

    #[test]
    fn truncates_success_output_when_over_limit() {
        let output = format_process_output(true, b"abcdef", b"", 3);
        assert_eq!(output, "abc...[truncated]");
    }

    #[test]
    fn prefixes_and_truncates_error_output() {
        let output = format_process_output(false, b"", b"failure details", 12);
        assert!(output.starts_with("Error: "));
        assert!(output.ends_with("...[truncated]"));
    }

    #[tokio::test]
    async fn run_process_enforces_timeout() {
        let mut perm = ShellPermission::default();
        perm.allow = true;
        perm.timeout = Some("1ms".to_string());

        let err = run_process(
            "sh".to_string(),
            vec!["-c".to_string(), "sleep 1".to_string()],
            perm,
            None,
        )
        .await
        .expect_err("must timeout");

        assert!(err.to_string().contains("Shell command timed out"));
    }

    #[tokio::test]
    #[cfg(not(target_os = "macos"))]
    async fn run_process_respects_working_directory() {
        let dir = tempdir().expect("tempdir");
        let expected = std::fs::canonicalize(dir.path()).expect("canonicalize");

        let mut perm = ShellPermission::default();
        perm.allow = true;
        perm.working_dir = Some(expected.to_string_lossy().to_string());

        let out = run_process(
            "sh".to_string(),
            vec!["-c".to_string(), "pwd".to_string()],
            perm,
            None,
        )
        .await
        .expect("run process");

        assert_eq!(out.trim(), expected.to_string_lossy());
    }
}
