use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use drolin_core::{BootPlan, MarkerScanner, SessionState, TransitionError, Validation};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    pub timeout: Option<Duration>,
    pub marker: Option<String>,
    pub log_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub state: SessionState,
    pub marker_seen: bool,
    pub log_path: Option<String>,
    pub elapsed_ms: u128,
}

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("the boot plan is invalid: {0}")]
    Invalid(String),
    #[error("cannot start {path}: {source}")]
    Spawn {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("cannot observe the guest process: {0}")]
    Io(#[from] io::Error),
    #[error("invalid session transition: {0}")]
    State(#[from] TransitionError),
}

/// Start a guest, forward its output, and wait for it.
///
/// The launcher owns no policy: the executable and arguments come from
/// `drolin-core`. This function adds only the host-specific parts -- spawning,
/// tee-ing to a log, killing at the deadline, and reading the marker.
pub fn run(plan: &BootPlan, options: &LaunchOptions) -> Result<Outcome, LaunchError> {
    let facts = crate::host::host_facts();
    let validation: Validation = plan.spec.validate_on(Some(&facts));
    if !validation.is_ok() {
        let details = validation
            .errors()
            .map(|issue| format!("{}: {}", issue.code, issue.message))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(LaunchError::Invalid(details));
    }

    let log = match &options.log_path {
        Some(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = File::create(path)?;
            writeln!(file, "# {}", plan.command_line())?;
            Some(Arc::new(Mutex::new(file)))
        }
        None => None,
    };

    let mut command = Command::new(&plan.executable);
    command
        .args(&plan.args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|source| LaunchError::Spawn {
        path: plan.executable.clone(),
        source,
    })?;

    let scanner = options
        .marker
        .as_ref()
        .map(|marker| Arc::new(Mutex::new(MarkerScanner::new(marker.clone()))));

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_thread = stdout.map(|stream| {
        let scanner = scanner.clone();
        let log = log.clone();
        std::thread::spawn(move || tee(stream, false, scanner, log))
    });
    let stderr_thread = stderr.map(|stream| {
        let scanner = scanner.clone();
        let log = log.clone();
        std::thread::spawn(move || tee(stream, true, scanner, log))
    });

    let started = Instant::now();
    let (status, timed_out) = wait_with_timeout(&mut child, options.timeout)?;
    if let Some(thread) = stdout_thread {
        let _ = thread.join();
    }
    if let Some(thread) = stderr_thread {
        let _ = thread.join();
    }

    let elapsed = started.elapsed();
    let marker_seen = scanner
        .as_ref()
        .map(|scanner| scanner.lock().expect("marker scanner").seen())
        .unwrap_or(false);
    let mut state = SessionState::Idle.start()?.mark_running()?;
    state = if timed_out {
        state.fail(format!(
            "timed out after {} s",
            options.timeout.unwrap_or_default().as_secs()
        ))?
    } else {
        state.stop(status)?
    };

    Ok(Outcome {
        state,
        marker_seen,
        log_path: options
            .log_path
            .as_ref()
            .map(|path| path.display().to_string()),
        elapsed_ms: elapsed.as_millis(),
    })
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Option<Duration>,
) -> io::Result<(Option<i32>, bool)> {
    let Some(limit) = timeout else {
        let status = child.wait()?;
        return Ok((status.code(), false));
    };
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok((status.code(), false));
        }
        if started.elapsed() >= limit {
            let _ = child.kill();
            let status = child.wait()?;
            return Ok((status.code(), true));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn tee(
    mut stream: impl Read,
    is_stderr: bool,
    scanner: Option<Arc<Mutex<MarkerScanner>>>,
    log: Option<Arc<Mutex<File>>>,
) {
    let mut buffer = [0u8; 8192];
    loop {
        let read = match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let chunk = &buffer[..read];
        if is_stderr {
            let _ = io::stderr().write_all(chunk);
            let _ = io::stderr().flush();
        } else {
            let _ = io::stdout().write_all(chunk);
            let _ = io::stdout().flush();
        }
        if let Some(scanner) = &scanner {
            scanner.lock().expect("marker scanner").push(chunk);
        }
        if let Some(log) = &log {
            if let Ok(mut file) = log.lock() {
                let _ = file.write_all(chunk);
                let _ = file.flush();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drolin_core::{BootPlan, GuestSpec};

    fn shell_plan(script: &str) -> BootPlan {
        BootPlan {
            executable: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            spec: GuestSpec {
                kernel: "/bin/sh".into(),
                ..GuestSpec::default()
            },
        }
    }

    #[test]
    fn captures_exit_status_and_marker() {
        let plan = shell_plan("echo before; echo DROLIN_GUEST_OK; exit 3");
        let outcome = run(
            &plan,
            &LaunchOptions {
                marker: Some("DROLIN_GUEST_OK".into()),
                ..LaunchOptions::default()
            },
        )
        .unwrap();
        assert!(outcome.marker_seen);
        assert_eq!(outcome.state, SessionState::Stopped { exit_code: Some(3) });
    }

    #[test]
    fn reports_a_missing_marker() {
        let plan = shell_plan("echo nothing");
        let outcome = run(
            &plan,
            &LaunchOptions {
                marker: Some("DROLIN_GUEST_OK".into()),
                ..LaunchOptions::default()
            },
        )
        .unwrap();
        assert!(!outcome.marker_seen);
        assert_eq!(outcome.state, SessionState::Stopped { exit_code: Some(0) });
    }

    #[test]
    fn times_out_and_kills_the_child() {
        let plan = shell_plan("sleep 5");
        let outcome = run(
            &plan,
            &LaunchOptions {
                timeout: Some(Duration::from_millis(100)),
                ..LaunchOptions::default()
            },
        )
        .unwrap();
        assert!(matches!(outcome.state, SessionState::Failed { .. }));
        assert!(outcome.elapsed_ms < 3000);
    }

    #[test]
    fn writes_the_log_and_the_command() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("guest.log");
        let plan = shell_plan("echo hello");
        let outcome = run(
            &plan,
            &LaunchOptions {
                log_path: Some(log_path.clone()),
                ..LaunchOptions::default()
            },
        )
        .unwrap();
        let log = std::fs::read_to_string(&log_path).unwrap();
        assert!(log.contains("# /bin/sh -c 'echo hello'"));
        assert!(log.contains("hello"));
        assert_eq!(outcome.log_path, Some(log_path.display().to_string()));
    }

    #[test]
    fn rejects_an_invalid_plan() {
        let mut plan = shell_plan("true");
        plan.spec.kernel.clear();
        assert!(matches!(run(&plan, &LaunchOptions::default()), Err(LaunchError::Invalid(_))));
    }
}
