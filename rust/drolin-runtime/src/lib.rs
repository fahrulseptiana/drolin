//! Host adapters for the drolin engine.
//!
//! Every module here touches the outside world exactly once: the ELF prober
//! reads a binary, the image store reads and writes files, the launcher spawns
//! a process. The policy they apply comes from `drolin-core`.

pub mod doctor;
pub mod elf;
pub mod host;
pub mod image;
pub mod launch;
pub mod net;
pub mod paths;

pub use doctor::{Check, Diagnosis, Status};
pub use elf::{ElfError, KernelInfo};
pub use host::host_facts;
pub use image::{ImageError, ImageStore};
pub use launch::{LaunchError, LaunchOptions, Outcome};
pub use net::{NetError, Network};
pub use paths::DataRoot;
