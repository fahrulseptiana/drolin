//! Pure domain model for launching a User-Mode Linux guest.
//!
//! This crate performs no I/O. Everything here is deterministic so that the
//! command line the guest receives can be tested without a kernel, a host, or
//! a filesystem. Adapters that touch the outside world live in
//! `drolin-runtime`.

mod boot;
mod image;
mod marker;
mod session;
mod spec;
mod target;

pub use boot::{BootPlan, KernelArg};
pub use image::{Compression, ImageRef, ImageSet};
pub use marker::MarkerScanner;
pub use session::{SessionState, TransitionError};
pub use spec::{BootMode, ConsoleMode, GuestSpec, Issue, Severity, Share, Validation};
pub use target::{Arch, HostFacts};
