//! The smallest useful guest init.
//!
//! It prints a marker, optionally runs a command, and powers the machine off.
//! It exists so the engine can be exercised end to end without a distribution
//! image, a shell, or a writable disk. Build it for a Linux target and pack it
//! as `/init` in an initramfs.

#[cfg(target_os = "linux")]
use std::ffi::CString;
use std::process::Command;

const MARKER: &str = "DROLIN_GUEST_OK";

fn main() {
    println!("{MARKER}");
    println!(
        "drolin-init: pid={} arch={}",
        std::process::id(),
        std::env::consts::ARCH
    );

    best_effort_mounts();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--exec") => {
            let Some(program) = args.get(1) else {
                eprintln!("drolin-init: --exec needs a program");
                power_off();
            };
            match Command::new(program).args(&args[2..]).status() {
                Ok(status) => println!("drolin-init: {program} exited with {status}"),
                Err(error) => eprintln!("drolin-init: cannot run {program}: {error}"),
            }
        }
        Some("--shell") if std::path::Path::new("/bin/sh").exists() => {
            let _ = Command::new("/bin/sh").status();
        }
        Some(other) => eprintln!("drolin-init: unknown command {other}"),
        None => {}
    }

    power_off();
}

#[cfg(target_os = "linux")]
fn best_effort_mounts() {
    for (source, target, kind) in [
        ("proc", "/proc", "proc"),
        ("sysfs", "/sys", "sysfs"),
        ("devtmpfs", "/dev", "devtmpfs"),
    ] {
        let _ = std::fs::create_dir_all(target);
        let (Ok(source), Ok(target), Ok(kind)) = (
            CString::new(source),
            CString::new(target),
            CString::new(kind),
        ) else {
            continue;
        };
        unsafe {
            libc::mount(
                source.as_ptr(),
                target.as_ptr(),
                kind.as_ptr(),
                0,
                std::ptr::null(),
            );
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn best_effort_mounts() {}

#[cfg(target_os = "linux")]
fn power_off() -> ! {
    unsafe {
        libc::sync();
        libc::reboot(libc::RB_POWER_OFF);
    }
    // A UML guest whose host refuses the reboot syscall must still terminate
    // this process; the kernel is configured with panic=-1 and will report it.
    eprintln!("drolin-init: power off failed");
    std::process::exit(1)
}

#[cfg(not(target_os = "linux"))]
fn power_off() -> ! {
    eprintln!("drolin-init: this build is not a Linux binary; build for a Linux target");
    std::process::exit(1)
}
