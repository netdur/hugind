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
    if !perm.allow {
        return Err(rquickjs::Error::new_loading_message(
            "Shell Error",
            "Shell execution is disabled.",
        ));
    }

    if let Some(whitelist) = &perm.whitelist {
        if !whitelist.iter().any(|cmd| cmd == &program) {
            return Err(rquickjs::Error::new_loading_message(
                "Shell Error",
                format!("Command '{}' is not whitelisted.", program),
            ));
        }
    }

    if let Some(blacklist) = &perm.blacklist {
        if blacklist.iter().any(|cmd| cmd == &program) {
            return Err(rquickjs::Error::new_loading_message(
                "Shell Error",
                format!("Command '{}' is blacklisted.", program),
            ));
        }
    }

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

    let result_str = if output.status.success() {
        if output.stdout.len() > max_len {
            let mut s = String::from_utf8_lossy(&output.stdout[..max_len]).to_string();
            s.push_str("...[truncated]");
            s
        } else {
            String::from_utf8_lossy(&output.stdout).to_string()
        }
    } else {
        let mut s = format!("Error: {}", String::from_utf8_lossy(&output.stderr));
        if s.len() > max_len {
            let mut actual_len = max_len;
            while !s.is_char_boundary(actual_len) {
                actual_len -= 1;
            }
            s.truncate(actual_len);
            s.push_str("...[truncated]");
        }
        s
    };

    Ok(result_str)
}

async fn run_command_inner(
    cmd_str: String,
    perm: ShellPermission,
    logger: Option<RunLogger>,
) -> Result<String> {
    let parts: Vec<&str> = cmd_str.split_whitespace().collect();
    if parts.is_empty() {
        return Err(rquickjs::Error::new_loading_message(
            "Shell Error",
            "Empty command",
        ));
    }
    let program = parts[0].to_string();
    let args = parts[1..].iter().map(|s| s.to_string()).collect();

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
