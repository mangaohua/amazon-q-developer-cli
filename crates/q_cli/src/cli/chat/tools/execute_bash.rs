use std::collections::VecDeque;
use std::io::Write;
use std::process::Stdio;

use crossterm::style::{
    self,
    Color,
};
use crossterm::{
    execute,
    queue,
};
use eyre::{
    Context as EyreContext,
    Result,
};
use fig_os_shim::Context;
use serde::Deserialize;
use tokio::io::AsyncBufReadExt;
use tokio::select;
use tracing::error;

use super::{
    InvokeOutput,
    MAX_TOOL_RESPONSE_SIZE,
    OutputKind,
};
use crate::cli::chat::truncate_safe;

#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteBash {
    pub command: String,
}

impl ExecuteBash {
    pub async fn invoke(&self, mut updates: impl Write) -> Result<InvokeOutput> {
        execute!(
            updates,
            style::SetForegroundColor(Color::Green),
            style::Print(format!("Executing `{}`", &self.command)),
            style::ResetColor,
            style::Print("\n"),
        )?;

        // We need to maintain a handle on stderr and stdout, but pipe it to the terminal as well
        let mut child = tokio::process::Command::new("bash")
            .arg("-c")
            .arg(&self.command)
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .wrap_err_with(|| format!("Unable to spawn command '{}'", &self.command))?;

        let stdout = child.stdout.take().unwrap();
        let stdout = tokio::io::BufReader::new(stdout);
        let mut stdout = stdout.lines();

        let stderr = child.stderr.take().unwrap();
        let stderr = tokio::io::BufReader::new(stderr);
        let mut stderr = stderr.lines();

        const LINE_COUNT: usize = 1024;
        let mut stdout_buf = VecDeque::with_capacity(LINE_COUNT);
        let mut stderr_buf = VecDeque::with_capacity(LINE_COUNT);

        let exit_status = loop {
            child.stdin.take();

            select! {
                biased;
                line = stdout.next_line() => match line {
                    Ok(Some(line)) => {
                        writeln!(updates, "{line}")?;
                        if stdout_buf.len() >= LINE_COUNT {
                            stdout_buf.pop_front();
                        }
                        stdout_buf.push_back(line);
                    },
                    Ok(None) => {},
                    Err(err) => error!(%err, "Failed to read stdout of child process"),
                },
                line = stderr.next_line() => match line {
                    Ok(Some(line)) => {
                        writeln!(updates, "{line}")?;
                        if stderr_buf.len() >= LINE_COUNT {
                            stderr_buf.pop_front();
                        }
                        stderr_buf.push_back(line);
                    },
                    Ok(None) => {},
                    Err(err) => error!(%err, "Failed to read stderr of child process"),
                },
                exit_status = child.wait() => {
                    break exit_status;
                },
            };
        }
        .wrap_err_with(|| format!("No exit status for '{}'", &self.command))?;

        updates.flush()?;

        let stdout = stdout_buf.into_iter().collect::<Vec<_>>().join("\n");
        let stderr = stderr_buf.into_iter().collect::<Vec<_>>().join("\n");

        let output = serde_json::json!({
            "exit_status": exit_status.code().unwrap_or(0).to_string(),
            "stdout": format!(
                "{}{}",
                truncate_safe(&stdout, MAX_TOOL_RESPONSE_SIZE / 3),
                if stdout.len() > MAX_TOOL_RESPONSE_SIZE / 3 {
                    " ... truncated"
                } else {
                    ""
                }
            ),
            "stderr": format!(
                "{}{}",
                truncate_safe(&stderr, MAX_TOOL_RESPONSE_SIZE / 3),
                if stderr.len() > MAX_TOOL_RESPONSE_SIZE / 3 {
                    " ... truncated"
                } else {
                    ""
                }
            ),
        });

        Ok(InvokeOutput {
            output: OutputKind::Json(output),
        })
    }

    pub fn queue_description(&self, updates: &mut impl Write) -> Result<()> {
        queue!(updates, style::Print("I will run the following shell command: "),)?;

        // TODO: Could use graphemes for a better heuristic
        if self.command.len() > 20 {
            queue!(updates, style::Print("\n"),)?;
        }

        Ok(queue!(
            updates,
            style::SetForegroundColor(Color::Green),
            style::Print(&self.command),
            style::ResetColor
        )?)
    }

    pub async fn validate(&mut self, _ctx: &Context) -> Result<()> {
        // TODO: probably some small amount of PATH checking
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_bash_tool() {
        let mut stdout = std::io::stdout();

        // Verifying stdout
        let v = serde_json::json!({
            "command": "echo Hello, world!",
        });
        let out = serde_json::from_value::<ExecuteBash>(v)
            .unwrap()
            .invoke(&mut stdout)
            .await
            .unwrap();

        if let OutputKind::Json(json) = out.output {
            assert_eq!(json.get("exit_status").unwrap(), &0.to_string());
            assert_eq!(json.get("stdout").unwrap(), "Hello, world!");
            assert_eq!(json.get("stderr").unwrap(), "");
        } else {
            panic!("Expected JSON output");
        }

        // Verifying stderr
        let v = serde_json::json!({
            "command": "echo Hello, world! 1>&2",
            "interactive": false
        });
        let out = serde_json::from_value::<ExecuteBash>(v)
            .unwrap()
            .invoke(&mut stdout)
            .await
            .unwrap();

        if let OutputKind::Json(json) = out.output {
            assert_eq!(json.get("exit_status").unwrap(), &0.to_string());
            assert_eq!(json.get("stdout").unwrap(), "");
            assert_eq!(json.get("stderr").unwrap(), "Hello, world!");
        } else {
            panic!("Expected JSON output");
        }

        // Verifying exit code
        let v = serde_json::json!({
            "command": "exit 1",
            "interactive": false
        });
        let out = serde_json::from_value::<ExecuteBash>(v)
            .unwrap()
            .invoke(&mut stdout)
            .await
            .unwrap();
        if let OutputKind::Json(json) = out.output {
            assert_eq!(json.get("exit_status").unwrap(), &1.to_string());
            assert_eq!(json.get("stdout").unwrap(), "");
            assert_eq!(json.get("stderr").unwrap(), "");
        } else {
            panic!("Expected JSON output");
        }
    }
}
