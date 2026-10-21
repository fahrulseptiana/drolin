use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    /// Give the guest a pseudoterminal and bridge it to this process's stdio.
    /// This is what makes an interactive shell work from an Android app, where
    /// a pipe console is unreliable.
    pub pty: bool,
    /// Hand the launcher's own stdio straight to the guest and do not capture.
    /// Used when the parent is already a terminal (Termux's TerminalSession).
    pub inherit: bool,
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
/// The guest's console is attached to a regular file rather than a pipe. UML's
/// fd console uses poll/epoll on its descriptors; on Android a pipe makes guest
/// writes stall, while a file works. Tailing the file gives the same live
/// output and makes marker detection independent of host pipe semantics.
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

    if options.inherit {
        let mut command = Command::new(&plan.executable);
        command
            .args(&plan.args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let mut child = command.spawn().map_err(|source| LaunchError::Spawn {
            path: plan.executable.clone(),
            source,
        })?;
        let started = Instant::now();
        let (status, timed_out) = wait_with_timeout(&mut child, options.timeout)?;
        let mut state = SessionState::Idle.start()?.mark_running()?;
        state = if timed_out {
            state.fail(format!(
                "timed out after {} s",
                options.timeout.unwrap_or_default().as_secs()
            ))?
        } else {
            state.stop(status)?
        };
        return Ok(Outcome {
            state,
            marker_seen: false,
            log_path: None,
            elapsed_ms: started.elapsed().as_millis(),
        });
    }

    static SESSION: AtomicU64 = AtomicU64::new(0);
    let log_path = options.log_path.clone().unwrap_or_else(|| {
        let session = SESSION.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("drolin-{}-{session}.log", std::process::id()))
    });
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut log = File::create(&log_path)?;
    writeln!(log, "# {}", plan.command_line())?;
    log.flush()?;
    let header_len = log.metadata()?.len();

    let scanner = options
        .marker
        .as_ref()
        .map(|marker| Arc::new(Mutex::new(MarkerScanner::new(marker.clone()))));

    let mut command = Command::new(&plan.executable);
    command.args(&plan.args);

    let done = Arc::new(AtomicBool::new(false));
    if options.pty {
        let (master, slave) = open_pty()?;
        let slave_in = slave.try_clone()?;
        let slave_out = slave.try_clone()?;
        command
            .stdin(Stdio::from(slave_in))
            .stdout(Stdio::from(slave_out))
            .stderr(Stdio::from(slave));
        let mut child = command.spawn().map_err(|source| LaunchError::Spawn {
            path: plan.executable.clone(),
            source,
        })?;

        let read_master = master.try_clone()?;
        let write_master = master;
        let log = Arc::new(Mutex::new(log));
        let output = {
            let scanner = scanner.clone();
            let log = log.clone();
            let done = done.clone();
            std::thread::spawn(move || pump_master(read_master, scanner, log, done))
        };
        std::thread::spawn(move || pump_stdin(write_master));

        let started = Instant::now();
        let (status, timed_out) = wait_with_timeout(&mut child, options.timeout)?;
        done.store(true, Ordering::Release);
        let _ = output.join();
        let elapsed = started.elapsed();
        finish(
            &scanner, &log_path, &log, status, timed_out, elapsed, options,
        )
    } else {
        let child_out = log.try_clone()?;
        let child_err = log.try_clone()?;
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::from(child_out))
            .stderr(Stdio::from(child_err));
        let mut child = command.spawn().map_err(|source| LaunchError::Spawn {
            path: plan.executable.clone(),
            source,
        })?;
        let tail_path = log_path.clone();
        let done_for_tail = done.clone();
        let scanner_for_tail = scanner.clone();
        let reader = std::thread::spawn(move || {
            tail(&tail_path, header_len, scanner_for_tail, done_for_tail)
        });
        let started = Instant::now();
        let (status, timed_out) = wait_with_timeout(&mut child, options.timeout)?;
        done.store(true, Ordering::Release);
        let _ = reader.join();
        let elapsed = started.elapsed();
        finish(
            &scanner,
            &log_path,
            &Arc::new(Mutex::new(log)),
            status,
            timed_out,
            elapsed,
            options,
        )
    }
}

fn finish(
    scanner: &Option<Arc<Mutex<MarkerScanner>>>,
    log_path: &std::path::Path,
    _log: &Arc<Mutex<File>>,
    status: Option<i32>,
    timed_out: bool,
    elapsed: Duration,
    options: &LaunchOptions,
) -> Result<Outcome, LaunchError> {
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
        log_path: Some(log_path.display().to_string()),
        elapsed_ms: elapsed.as_millis(),
    })
}

fn open_pty() -> io::Result<(File, File)> {
    use std::os::fd::FromRawFd;
    let mut master = -1;
    let mut slave = -1;
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    unsafe { Ok((File::from_raw_fd(master), File::from_raw_fd(slave))) }
}

fn pump_master(
    mut master: File,
    scanner: Option<Arc<Mutex<MarkerScanner>>>,
    log: Arc<Mutex<File>>,
    done: Arc<AtomicBool>,
) {
    use std::os::fd::AsRawFd;
    unsafe {
        libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
    }
    let mut buffer = [0u8; 8192];
    loop {
        match master.read(&mut buffer) {
            Ok(0) => return,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.raw_os_error() == Some(libc::EAGAIN) =>
            {
                if done.load(Ordering::Acquire) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(read) => {
                let chunk = &buffer[..read];
                let _ = io::stdout().write_all(chunk);
                let _ = io::stdout().flush();
                if let Some(scanner) = &scanner {
                    scanner.lock().expect("marker scanner").push(chunk);
                }
                if let Ok(mut file) = log.lock() {
                    let _ = file.write_all(chunk);
                    let _ = file.flush();
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        }
    }
}

fn pump_stdin(mut master: File) {
    let mut stdin = io::stdin();
    let mut buffer = [0u8; 4096];
    loop {
        match stdin.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => {
                if master.write_all(&buffer[..read]).is_err() {
                    return;
                }
                let _ = master.flush();
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        }
    }
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

fn tail(
    path: &std::path::Path,
    start: u64,
    scanner: Option<Arc<Mutex<MarkerScanner>>>,
    done: Arc<AtomicBool>,
) {
    let Ok(mut file) = File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::Start(start)).is_err() {
        return;
    }
    let mut buffer = [0u8; 8192];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => {
                if done.load(Ordering::Acquire) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(read) => {
                let chunk = &buffer[..read];
                let _ = io::stdout().write_all(chunk);
                let _ = io::stdout().flush();
                if let Some(scanner) = &scanner {
                    scanner.lock().expect("marker scanner").push(chunk);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return,
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
        assert!(matches!(
            run(&plan, &LaunchOptions::default()),
            Err(LaunchError::Invalid(_))
        ));
    }
}
