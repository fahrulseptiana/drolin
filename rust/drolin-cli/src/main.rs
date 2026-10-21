//! `drolin` is a thin command-line adapter over `drolin-core` and
//! `drolin-runtime`. It parses flags, prints results, and chooses an exit code;
//! it does not build kernel arguments itself.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use drolin_core::{
    BootMode, BootPlan, ConsoleMode, GuestSpec, ImageSet, Share, Validation,
};
use drolin_runtime::doctor;
use drolin_runtime::elf;
use drolin_runtime::image::ImageStore;
use drolin_runtime::launch::{self, LaunchOptions};
use drolin_runtime::paths::DataRoot;

const USAGE: &str = "\
drolin -- run a User-Mode Linux guest

Usage:
  drolin plan   --kernel PATH [options] [--json]
  drolin run    --kernel PATH [options] [--timeout SECS] [--marker TEXT] [--log PATH]
  drolin doctor --kernel PATH [options] [--json]
  drolin kernel inspect --kernel PATH [--json]
  drolin images list [--json]
  drolin images verify --manifest FILE --name NAME
  drolin images install --manifest FILE --name NAME --source FILE
  drolin version

Boot options:
  --kernel PATH             User-Mode Linux kernel (required)
  --initramfs PATH          initramfs image
  --rootfs PATH             root filesystem image, attached as /dev/ubda
  --data PATH               second disk, attached as /dev/ubdb
  --share PATH              host directory exposed through hostfs
  --memory MIB              guest memory, default 512
  --cpus N                  guest CPUs, default 1
  --init PATH               guest PID 1 (absolute)
  --stub-exe PATH           standalone stub binary for hosts that reject memfd exec
  --console MODE            stdio (default), pty, none
  --mode MODE               ptrace (default), nosysemu, seccomp
  --guest-page-size KIB     tell the planner the guest page size
  --arg VALUE               extra kernel argument (repeatable)
  --json                    machine-readable output

Run options:
  --timeout SECS            kill the guest after SECS (0 disables)
  --marker TEXT             success marker to scan for in the log
  --log PATH                write the guest's output to PATH
  --pty                     give the guest a pseudoterminal

Exit codes:
  0 success, 1 guest/check failure, 2 usage, 3 invalid plan, 4 launch error.
";

fn main() -> ExitCode {
    match run_cli(std::env::args().skip(1).collect()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("drolin: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    Invalid(Validation),
    Runtime(String),
}

impl CliError {
    fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Invalid(_) => 3,
            Self::Runtime(_) => 4,
        }
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(message) => f.write_str(message),
            Self::Invalid(validation) => {
                for issue in validation.errors() {
                    writeln!(f, "{}: {}", issue.code, issue.message)?;
                }
                Ok(())
            }
            Self::Runtime(message) => f.write_str(message),
        }
    }
}

#[derive(Debug, Clone)]
struct Common {
    spec: GuestSpec,
    json: bool,
}

impl Default for Common {
    fn default() -> Self {
        Self {
            spec: GuestSpec::default(),
            json: false,
        }
    }
}

#[derive(Debug, Clone)]
enum Parsed {
    Help,
    Version,
    Plan(Common),
    Run(Common, RunOptions),
    Doctor(Common),
    KernelInspect { kernel: PathBuf, json: bool },
    ImagesList { json: bool },
    ImagesVerify { manifest: PathBuf, name: String },
    ImagesInstall { manifest: PathBuf, name: String, source: PathBuf },
}

#[derive(Debug, Clone, Default)]
struct RunOptions {
    timeout: Option<Duration>,
    marker: Option<String>,
    log_path: Option<PathBuf>,
    pty: bool,
}

fn run_cli(args: Vec<String>) -> Result<u8, CliError> {
    let parsed = parse(args)?;
    match parsed {
        Parsed::Help => {
            print!("{USAGE}");
            Ok(0)
        }
        Parsed::Version => {
            println!("drolin {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Parsed::Plan(common) => cmd_plan(common),
        Parsed::Run(common, options) => cmd_run(common, options),
        Parsed::Doctor(common) => cmd_doctor(common),
        Parsed::KernelInspect { kernel, json } => cmd_kernel_inspect(&kernel, json),
        Parsed::ImagesList { json } => cmd_images_list(json),
        Parsed::ImagesVerify { manifest, name } => cmd_images_verify(&manifest, &name),
        Parsed::ImagesInstall { manifest, name, source } => {
            cmd_images_install(&manifest, &name, &source)
        }
    }
}

fn parse(args: Vec<String>) -> Result<Parsed, CliError> {
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(Parsed::Help);
    };
    if matches!(command, "-h" | "--help" | "help") {
        return Ok(Parsed::Help);
    }
    if command == "version" || command == "--version" {
        return Ok(Parsed::Version);
    }
    if command == "kernel" {
        return parse_kernel(&args[1..]);
    }
    if command == "images" {
        return parse_images(&args[1..]);
    }
    let rest = &args[1..];
    match command {
        "plan" => {
            let (common, _) = parse_common(rest)?;
            Ok(Parsed::Plan(common))
        }
        "run" => {
            let (common, run) = parse_common(rest)?;
            Ok(Parsed::Run(common, run))
        }
        "doctor" => {
            let (common, _) = parse_common(rest)?;
            Ok(Parsed::Doctor(common))
        }
        other => Err(CliError::Usage(format!(
            "unknown command {other:?}; try `drolin --help`"
        ))),
    }
}

fn parse_kernel(args: &[String]) -> Result<Parsed, CliError> {
    if args.first().map(String::as_str) != Some("inspect") {
        return Err(CliError::Usage(
            "usage: drolin kernel inspect --kernel PATH [--json]".into(),
        ));
    }
    let mut kernel = None;
    let mut json = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--kernel" => kernel = Some(PathBuf::from(take_value(args, &mut index)?)),
            "--json" => json = true,
            "-h" | "--help" => return Ok(Parsed::Help),
            other => return Err(CliError::Usage(format!("unknown option {other:?}"))),
        }
        index += 1;
    }
    let kernel = kernel.ok_or_else(|| CliError::Usage("--kernel is required".into()))?;
    Ok(Parsed::KernelInspect { kernel, json })
}

fn parse_images(args: &[String]) -> Result<Parsed, CliError> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return Err(CliError::Usage(
            "usage: drolin images <list|verify|install> ...".into(),
        ));
    };
    let mut manifest = None;
    let mut name = None;
    let mut source = None;
    let mut json = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--manifest" => manifest = Some(PathBuf::from(take_value(args, &mut index)?)),
            "--name" => name = Some(take_value(args, &mut index)?),
            "--source" => source = Some(PathBuf::from(take_value(args, &mut index)?)),
            "--json" => json = true,
            "-h" | "--help" => return Ok(Parsed::Help),
            other => return Err(CliError::Usage(format!("unknown option {other:?}"))),
        }
        index += 1;
    }
    match subcommand {
        "list" => Ok(Parsed::ImagesList { json }),
        "verify" => Ok(Parsed::ImagesVerify {
            manifest: manifest.ok_or_else(|| CliError::Usage("--manifest is required".into()))?,
            name: name.ok_or_else(|| CliError::Usage("--name is required".into()))?,
        }),
        "install" => Ok(Parsed::ImagesInstall {
            manifest: manifest.ok_or_else(|| CliError::Usage("--manifest is required".into()))?,
            name: name.ok_or_else(|| CliError::Usage("--name is required".into()))?,
            source: source.ok_or_else(|| CliError::Usage("--source is required".into()))?,
        }),
        other => Err(CliError::Usage(format!("unknown images command {other:?}"))),
    }
}

fn parse_common(args: &[String]) -> Result<(Common, RunOptions), CliError> {
    let mut common = Common::default();
    let mut run = RunOptions::default();
    let mut index = 0;
    let mut after_separator = false;
    while index < args.len() {
        let arg = &args[index];
        if after_separator {
            common.spec.extra_args.push(arg.clone());
            index += 1;
            continue;
        }
        match arg.as_str() {
            "--" => after_separator = true,
            "-h" | "--help" => return Ok((common, run)),
            "--json" => common.json = true,
            "--kernel" => common.spec.kernel = take_value(args, &mut index)?,
            "--initramfs" => common.spec.initramfs = Some(take_value(args, &mut index)?),
            "--rootfs" => common.spec.rootfs = Some(take_value(args, &mut index)?),
            "--data" => common.spec.data_disk = Some(take_value(args, &mut index)?),
            "--share" => {
                common.spec.share = Some(Share {
                    host_path: take_value(args, &mut index)?,
                })
            }
            "--memory" => common.spec.memory_mib = parse_number(&take_value(args, &mut index)?, "--memory")?,
            "--cpus" => common.spec.cpus = parse_number(&take_value(args, &mut index)?, "--cpus")?,
            "--init" => common.spec.init = Some(take_value(args, &mut index)?),
            "--stub-exe" => common.spec.stub_exe = Some(take_value(args, &mut index)?),
            "--console" => {
                let value = take_value(args, &mut index)?;
                common.spec.console = match value.as_str() {
                    "stdio" => ConsoleMode::Stdio,
                    "pty" => ConsoleMode::Pty,
                    "none" => ConsoleMode::None,
                    other => return Err(CliError::Usage(format!("unknown console {other:?}"))),
                };
            }
            "--mode" => {
                let value = take_value(args, &mut index)?;
                common.spec.mode = match value.as_str() {
                    "ptrace" => BootMode::Ptrace,
                    "nosysemu" => BootMode::PtraceNoSysemu,
                    "seccomp" => BootMode::Seccomp,
                    other => return Err(CliError::Usage(format!("unknown mode {other:?}"))),
                };
            }
            "--guest-page-size" => {
                common.spec.guest_page_size_kib =
                    Some(parse_number(&take_value(args, &mut index)?, "--guest-page-size")?)
            }
            "--arg" => common.spec.extra_args.push(take_value(args, &mut index)?),
            "--timeout" => {
                let seconds: u64 = parse_number(&take_value(args, &mut index)?, "--timeout")?;
                run.timeout = if seconds == 0 {
                    None
                } else {
                    Some(Duration::from_secs(seconds))
                };
            }
            "--marker" => run.marker = Some(take_value(args, &mut index)?),
            "--log" => run.log_path = Some(PathBuf::from(take_value(args, &mut index)?)),
            "--pty" => run.pty = true,
            other => {
                return Err(CliError::Usage(format!(
                    "unknown option {other:?}; try `drolin --help`"
                )))
            }
        }
        index += 1;
    }
    Ok((common, run))
}

fn take_value(args: &[String], index: &mut usize) -> Result<String, CliError> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| CliError::Usage(format!("{} needs a value", args[*index - 1])))
}

fn parse_number<T: std::str::FromStr>(value: &str, option: &str) -> Result<T, CliError> {
    value
        .parse()
        .map_err(|_| CliError::Usage(format!("{option} expects a number, got {value:?}")))
}

fn cmd_plan(common: Common) -> Result<u8, CliError> {
    let validation = common.spec.validate();
    let plan = BootPlan::from_spec(common.spec);
    if !validation.is_ok() {
        if common.json {
            print_json(&serde_json::json!({
                "plan": plan,
                "validation": validation,
            }))?;
        } else {
            for issue in validation.errors() {
                eprintln!("{}: {}", issue.code, issue.message);
            }
        }
        return Ok(3);
    }
    if common.json {
        print_json(&serde_json::json!({
            "plan": plan,
            "validation": validation,
        }))?;
    } else {
        for issue in validation.warnings() {
            eprintln!("warning: {}: {}", issue.code, issue.message);
        }
        println!("{}", plan.command_line());
    }
    Ok(0)
}

fn cmd_run(common: Common, run: RunOptions) -> Result<u8, CliError> {
    let validation = common.spec.validate();
    if !validation.is_ok() {
        return Err(CliError::Invalid(validation));
    }
    let plan = BootPlan::from_spec(common.spec);
    if !common.json {
        eprintln!("booting: {}", plan.command_line());
    }
    let options = LaunchOptions {
        timeout: run.timeout,
        marker: run.marker,
        log_path: run.log_path,
        pty: run.pty,
    };
    let outcome = launch::run(&plan, &options).map_err(|error| CliError::Runtime(error.to_string()))?;
    if common.json {
        print_json(&outcome)?;
    } else {
        println!("{}", outcome.state);
        if options.marker.is_some() {
            println!(
                "marker: {}",
                if outcome.marker_seen { "found" } else { "missing" }
            );
        }
    }
    // A requested marker is the success criterion. A guest that stays up
    // (an interactive shell, or an image that has no poweroff tool) is not a
    // failure once it has proved it booted.
    let failed = if options.marker.is_some() {
        !outcome.marker_seen
    } else {
        !outcome.state.is_terminal()
            || matches!(outcome.state, drolin_core::SessionState::Failed { .. })
            || matches!(outcome.state, drolin_core::SessionState::Stopped { exit_code: Some(code) } if code != 0)
    };
    Ok(if failed { 1 } else { 0 })
}

fn cmd_doctor(common: Common) -> Result<u8, CliError> {
    let diagnosis = doctor::diagnose(&common.spec);
    if common.json {
        print_json(&diagnosis)?;
    } else {
        for check in &diagnosis.checks {
            let label = match check.status {
                drolin_runtime::Status::Pass => "PASS",
                drolin_runtime::Status::Warn => "WARN",
                drolin_runtime::Status::Fail => "FAIL",
            };
            println!("[{label}] {}: {}", check.name, check.detail);
        }
    }
    Ok(if diagnosis.ok() { 0 } else { 1 })
}

fn cmd_kernel_inspect(kernel: &std::path::Path, json: bool) -> Result<u8, CliError> {
    let info = elf::inspect(kernel).map_err(|error| CliError::Runtime(error.to_string()))?;
    if json {
        print_json(&serde_json::json!({
            "path": info.path.display().to_string(),
            "arch": info.arch.map(|arch| arch.as_str().to_owned()),
            "is_64_bit": info.is_64_bit,
            "is_little_endian": info.is_little_endian,
            "size_bytes": info.size_bytes,
        }))?;
    } else {
        println!("path: {}", info.path.display());
        println!(
            "arch: {}",
            info.arch
                .map(|arch| arch.as_str().to_owned())
                .unwrap_or_else(|| "unknown".into())
        );
        println!("bits: {}", if info.is_64_bit { 64 } else { 32 });
        println!("size: {} bytes", info.size_bytes);
    }
    Ok(0)
}

fn cmd_images_list(json: bool) -> Result<u8, CliError> {
    let data = DataRoot::from_env().map_err(|error| CliError::Runtime(error.to_string()))?;
    let store = ImageStore::new(&data);
    let files = store
        .list()
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    if json {
        print_json(&serde_json::json!({
            "root": store.root().display().to_string(),
            "files": files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        }))?;
    } else {
        println!("root: {}", store.root().display());
        for file in files {
            println!("{}", file.display());
        }
    }
    Ok(0)
}

fn load_manifest(path: &std::path::Path) -> Result<ImageSet, CliError> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| CliError::Runtime(format!("{}: {error}", path.display())))?;
    let set: ImageSet = serde_json::from_str(&text)
        .map_err(|error| CliError::Runtime(format!("{}: {error}", path.display())))?;
    let problems = set.validate();
    if !problems.is_empty() {
        return Err(CliError::Runtime(problems.join("; ")));
    }
    Ok(set)
}

fn cmd_images_verify(manifest: &std::path::Path, name: &str) -> Result<u8, CliError> {
    let set = load_manifest(manifest)?;
    let image = set
        .find(name)
        .ok_or_else(|| CliError::Runtime(format!("{} is not in the manifest", name)))?;
    let data = DataRoot::from_env().map_err(|error| CliError::Runtime(error.to_string()))?;
    let store = ImageStore::new(&data);
    store
        .verify(image)
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    println!("{}: verified", image.name);
    Ok(0)
}

fn cmd_images_install(
    manifest: &std::path::Path,
    name: &str,
    source: &std::path::Path,
) -> Result<u8, CliError> {
    let set = load_manifest(manifest)?;
    let image = set
        .find(name)
        .ok_or_else(|| CliError::Runtime(format!("{} is not in the manifest", name)))?;
    let data = DataRoot::from_env().map_err(|error| CliError::Runtime(error.to_string()))?;
    data.ensure()
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    let store = ImageStore::new(&data);
    let installed = store
        .install(image, source)
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    println!("{}", installed.display());
    Ok(0)
}

fn print_json(value: &impl serde::Serialize) -> Result<(), CliError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| CliError::Runtime(format!("cannot encode JSON: {error}")))?;
    println!("{text}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(args: &[&str]) -> Parsed {
        parse(args.iter().map(|s| s.to_string()).collect()).unwrap_err_or()
    }

    trait UnwrapParsed {
        fn unwrap_err_or(self) -> Parsed;
    }
    impl UnwrapParsed for Result<Parsed, CliError> {
        fn unwrap_err_or(self) -> Parsed {
            self.unwrap_or_else(|e| panic!("parse failed: {e}"))
        }
    }

    #[test]
    fn help_is_the_default() {
        assert!(matches!(parse(vec![]).unwrap(), Parsed::Help));
    }

    #[test]
    fn plan_parses_boot_options() {
        let parsed = parse_ok(&[
            "plan", "--kernel", "/k", "--memory", "1024", "--cpus", "2", "--mode", "seccomp",
            "--console", "pty", "--arg", "quiet",
        ]);
        let Parsed::Plan(common) = parsed else {
            panic!("not a plan")
        };
        assert_eq!(common.spec.kernel, "/k");
        assert_eq!(common.spec.memory_mib, 1024);
        assert_eq!(common.spec.cpus, 2);
        assert_eq!(common.spec.mode, BootMode::Seccomp);
        assert_eq!(common.spec.console, ConsoleMode::Pty);
        assert_eq!(common.spec.extra_args, ["quiet"]);
    }

    #[test]
    fn trailing_arguments_are_kernel_arguments() {
        let parsed = parse_ok(&["plan", "--kernel", "/k", "--", "quiet", "debug"]);
        let Parsed::Plan(common) = parsed else {
            panic!("not a plan")
        };
        assert_eq!(common.spec.extra_args, ["quiet", "debug"]);
    }

    #[test]
    fn run_parses_timeout_and_marker() {
        let parsed = parse_ok(&[
            "run", "--kernel", "/k", "--timeout", "30", "--marker", "OK", "--log", "/tmp/log",
        ]);
        let Parsed::Run(_, run) = parsed else {
            panic!("not a run")
        };
        assert_eq!(run.timeout, Some(Duration::from_secs(30)));
        assert_eq!(run.marker.as_deref(), Some("OK"));
        assert_eq!(run.log_path, Some(PathBuf::from("/tmp/log")));
    }

    #[test]
    fn zero_timeout_means_no_deadline() {
        let parsed = parse_ok(&["run", "--kernel", "/k", "--timeout", "0"]);
        let Parsed::Run(_, run) = parsed else {
            panic!("not a run")
        };
        assert_eq!(run.timeout, None);
    }

    #[test]
    fn unknown_options_are_usage_errors() {
        let error = parse(vec!["plan".into(), "--nope".into()]).unwrap_err();
        assert!(matches!(error, CliError::Usage(_)));
    }

    #[test]
    fn missing_values_are_usage_errors() {
        let error = parse(vec!["plan".into(), "--kernel".into()]).unwrap_err();
        assert!(matches!(error, CliError::Usage(_)));
    }

    #[test]
    fn bad_numbers_are_usage_errors() {
        let error = parse(vec![
            "plan".into(),
            "--kernel".into(),
            "/k".into(),
            "--memory".into(),
            "lots".into(),
        ])
        .unwrap_err();
        assert!(matches!(error, CliError::Usage(_)));
    }

    #[test]
    fn reserved_arguments_reach_validation() {
        let parsed = parse_ok(&["plan", "--kernel", "/k", "--arg", "mem=1"]);
        let Parsed::Plan(common) = parsed else {
            panic!("not a plan")
        };
        assert!(!common.spec.validate().is_ok());
    }
}
