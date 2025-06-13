// SPDX-FileCopyrightText: Copyright © 2025 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

use std::path::PathBuf;

use bootloader::systemd_boot;
use gpt::GptError;
use thiserror::Error;

mod kernel;
pub use kernel::{AuxiliaryFile, AuxiliaryKind, BootJSON, Kernel, Schema};

mod bootenv;
pub use bootenv::{BootEnvironment, Firmware};
pub mod bootloader;
pub mod os_release;

mod manager;
pub use manager::Manager;

/// Re-export the topology APIs
pub use topology::disk;

pub mod file_utils;

mod entry;

pub use entry::{CmdlineEntry, Entry};

/// Core error type for blsforme
#[derive(Debug, Error)]
pub enum Error {
    #[error("boot loader protocol: {0}")]
    BootLoaderProtocol(#[from] systemd_boot::interface::Error),

    #[error("bootloader error")]
    Bootloader(#[from] bootloader::Error),

    #[error("c stdlib: {0}")]
    C(#[from] nix::errno::Errno),

    #[error("undetected xbootldr")]
    NoXbootldr,

    #[error("undetected ESP")]
    NoEsp,

    #[error("failed to interact with filesystem properly")]
    InvalidFilesystem,

    #[error("generic i/o error")]
    Io(#[from] std::io::Error),

    #[error("GPT error")]
    Gpt(#[from] GptError),

    #[error("topology scan: {0}")]
    Topology(#[from] topology::disk::Error),

    #[error("no ESP mounted in image mode, but detected an ESP at {0}")]
    UnmountedEsp(PathBuf),

    #[error("unsupported usage")]
    Unsupported,
}

/// Core configuration for boot management
#[derive(Debug)]
pub struct Configuration {
    /// Root of all operations
    pub root: Root,

    /// Where we can find `sysfs` `proc` etc
    pub vfs: PathBuf,
}

/// Wrap a root into a strong type to avoid confusion
#[derive(Debug)]
pub enum Root {
    /// Native installation
    Native(PathBuf),

    /// Image generation
    Image(PathBuf),
}

impl Root {
    /// When we don't need the type of the root..
    pub fn path(&self) -> &PathBuf {
        match self {
            Root::Native(p) => p,
            Root::Image(p) => p,
        }
    }
}
