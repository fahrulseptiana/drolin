//! Minimal poweroff for guest images that do not ship systemd.
//!
//! Ubuntu base is a userland without the systemd tools, so an init script has
//! no `poweroff` binary to call. This is the same syscall with no dependencies.

#[cfg(target_os = "linux")]
fn main() {
    unsafe {
        libc::sync();
        libc::reboot(libc::RB_POWER_OFF);
    }
    eprintln!("drolin-poweroff: reboot failed");
    std::process::exit(1);
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("drolin-poweroff: build this for a Linux target");
    std::process::exit(1);
}
