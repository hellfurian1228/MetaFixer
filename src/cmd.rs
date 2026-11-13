use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use wait_timeout::ChildExt;

#[derive(Clone, Default)]
pub struct Log {
    lines: Arc<Mutex<Vec<String>>>,
}

impl Log {
    pub fn line(&self, text: impl Into<String>) {
        self.lines.lock().expect("log").push(text.into());
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.lines.lock().expect("log").clone()
    }
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(args: &[&str], log: Option<&Log>, timeout_secs: u64) -> RunResult {
    if let Some(log) = log {
        log.line(format!("$ {}", args.join(" ")));
    }
    if args.is_empty() {
        return RunResult {
            code: 127,
            stdout: String::new(),
            stderr: "empty command".into(),
        };
    }
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            let result = RunResult {
                code: 127,
                stdout: String::new(),
                stderr: format!("{} not found ({err})", args[0]),
            };
            if let Some(log) = log {
                log.line(result.stderr.clone());
            }
            return result;
        }
    };
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut pipe) = stdout_pipe.take() {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut pipe) = stderr_pipe.take() {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });
    let status = match child.wait_timeout(Duration::from_secs(timeout_secs)) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            let result = RunResult {
                code: -1,
                stdout: String::new(),
                stderr: "timed out".into(),
            };
            if let Some(log) = log {
                log.line("Timed out waiting.");
            }
            return result;
        }
        Err(err) => {
            return RunResult {
                code: 1,
                stdout: String::new(),
                stderr: err.to_string(),
            };
        }
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    if let Some(log) = log {
        let out = stdout.trim();
        let err = stderr.trim();
        if !out.is_empty() {
            log.line(out.to_string());
        }
        if !err.is_empty() {
            log.line(err.to_string());
        }
    }
    RunResult {
        code: status.code().unwrap_or(1),
        stdout,
        stderr,
    }
}

pub fn powershell(script: &str, log: Option<&Log>, timeout_secs: u64) -> RunResult {
    run(
        &[
            "powershell.exe",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
        log,
        timeout_secs,
    )
}

pub fn which(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}
