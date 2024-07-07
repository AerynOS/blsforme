// SPDX-FileCopyrightText: Copyright © 2024 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! Provides a CLI compatible with `clr-boot-manager` to be used as a drop-in
//! replacement for Solus.

use std::{
    fs::{self},
    path::{Path, PathBuf},
    str::FromStr,
};

use blsforme::{os_release::OsRelease, BootJSON, Configuration, Entry, Manager, Root, Schema};
use clap::{Parser, Subcommand};
use color_eyre::{
    eyre::{eyre, Ok},
    Section,
};

use pretty_env_logger::formatted_builder;

/// Boot Loader Specification compatible kernel/initrd/cmdline management
#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    /// Override base path for all boot management operations
    #[arg(short, long, global = true)]
    path: Option<PathBuf>,

    /// Force running in image mode (scripting integration)
    #[arg(short, long, global = true)]
    image: bool,

    /// Do not allow updating EFI vars
    #[arg(short, long, global = true)]
    no_efi_update: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Print version and exit
    Version,

    /// Report currently running kernel as successfully booting
    ReportBooted,

    /// Remove specified kernel from the system
    RemoveKernel,

    /// Mount the `$BOOT` directories
    MountBoot,

    /// Configure the `$BOOT` directories for next boot
    Update,

    /// Set the bootloader timeout value
    SetTimeout { timeout: u64 },

    /// Retrieve the bootloader timeout value
    GetTimeout,

    /// Set the kernel that will be used at next boot
    SetKernel { kernel: String },

    /// List kernels on `$BOOT`
    ListKernels,

    /// Status information (debugging)
    Status,
}

fn scan_os_release(root: impl AsRef<Path>) -> color_eyre::Result<OsRelease> {
    let root = root.as_ref();
    let query_paths = vec![
        root.join("run").join("os-release"),
        root.join("etc").join("os-release"),
        root.join("usr").join("lib").join("os-release"),
    ];

    for p in query_paths {
        if p.exists() {
            log::trace!("Reading os-release from: {}", p.display());
            let text = fs::read_to_string(p)?;
            let os_rel = OsRelease::from_str(&text)?;
            return Ok(os_rel);
        }
    }
    Err(eyre!(
        "Failed to determine the Linux distribution by scanning os-release"
    ))
}

/// Query the schema we need to use for pre BLS schema installations
fn query_schema(config: &Configuration) -> color_eyre::Result<Schema> {
    let os_rel = scan_os_release(config.root.path())?;

    match os_rel.id.as_str() {
        "solus" => {
            if os_rel.version.name.is_some_and(|v| v.starts_with("4.")) {
                log::trace!("Legacy schema due to Solus 4 installation");
                Ok(Schema::Legacy("com.solus-project"))
            } else {
                Ok(Schema::Blsforme)
            }
        }
        "clear-linux-os" => {
            log::trace!("Legacy schema due to Clear Linux OS installation");
            Ok(Schema::Legacy("org.clearlinux"))
        }
        _ => Ok(Schema::Blsforme),
    }
}

fn inspect_root(config: &Configuration) -> color_eyre::Result<()> {
    if let Err(e) = check_permissions() {
        log::error!("{:#}", e);
        return Ok(());
    }

    let schema = query_schema(config)?;
    log::info!("Root Schema: {schema:?}");

    let paths = glob::glob(&format!("{}/usr/lib/kernel/*", config.root.path().display()))?
        .chain(glob::glob(&format!(
            "{}/usr/lib/kernel/*/*",
            config.root.path().display()
        ))?)
        .filter_map(|f| f.ok());
    let mut kernels = schema.discover_system_kernels(paths)?;

    // Future: Include other potential bootloader asset paths
    let booty_bits = glob::glob(&format!(
        "{}/usr/lib*/systemd/boot/efi/*.efi",
        config.root.path().display()
    ))?
    .filter_map(|f| f.ok())
    .collect::<Vec<_>>();

    // If a boot JSON is provided, augment the records
    for kernel in kernels.iter_mut() {
        if let Some(json) = kernel
            .extras
            .iter()
            .find(|e| matches!(e.kind, blsforme::AuxilliaryKind::BootJSON))
        {
            let text = fs::read_to_string(&json.path)?;
            let decoded = BootJSON::try_from(text.as_str())?;
            kernel.variant = Some(decoded.variant.to_string());
        }
    }
    log::info!("Kernels: {kernels:?}");
    let entries = kernels.iter().map(Entry::new);

    // Query the manager
    let manager = Manager::new(config)?
        .with_entries(entries)
        .with_bootloader_assets(booty_bits);
    let _parts = manager.mount_partitions()?;
    eprintln!("manager = {manager:?}");

    Ok(())
}

/// Bail-out permission check for execution
fn check_permissions() -> color_eyre::Result<()> {
    let euid = unsafe { nix::libc::geteuid() };
    match euid {
        0 => Ok(()),
        _ => Err(eyre!("blsctl must be run with root privileges to work correctly"))
            .note("This tool must be able to mount partitions and scan partition tables to operate effectively"),
    }
}

fn main() -> color_eyre::Result<()> {
    let host_os = scan_os_release("/").expect("Cannot determine running Linux distro");
    color_eyre::config::HookBuilder::default()
        .issue_url("https://github.com/serpent-os/blsforme/issues/new")
        .add_issue_metadata("tool-context", "standalone (blsctl)")
        .add_issue_metadata("version", env!("CARGO_PKG_VERSION"))
        .add_issue_metadata("os-release-name", host_os.name)
        .add_issue_metadata("os-release-version", host_os.version.name.unwrap_or("n/a".into()))
        .issue_filter(|_| true)
        .install()?;

    formatted_builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();

    let res = Cli::parse();
    let root = if res.image {
        // forced image mode
        Root::Image(res.path.unwrap_or("/".into()))
    } else if let Some(path) = res.path {
        // Path provided, native only if it is `/`
        if path.as_path() == Path::new("/") {
            Root::Native(path)
        } else {
            Root::Image(path)
        }
    } else {
        // Native operation
        Root::Native("/".into())
    };

    let config = Configuration { root, vfs: "/".into() };

    log::trace!("Using configuration: {config:?}");
    log::info!("Inspecting root device: {}", config.root.path().display());

    match res.command {
        Commands::Version => todo!(),
        Commands::ReportBooted => todo!(),
        Commands::RemoveKernel => todo!(),
        Commands::MountBoot => todo!(),
        Commands::Update => todo!(),
        Commands::SetTimeout { timeout: _ } => todo!(),
        Commands::GetTimeout => todo!(),
        Commands::SetKernel { kernel: _ } => todo!(),
        Commands::ListKernels => todo!(),
        Commands::Status => {
            inspect_root(&config)?;
        }
    }

    Ok(())
}
