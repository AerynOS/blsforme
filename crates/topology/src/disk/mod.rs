// SPDX-FileCopyrightText: Copyright © 2025 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! Disk probe/query APIs

use std::path::PathBuf;

use thiserror::Error;

mod builder;
pub use builder::Builder;
pub mod device;
pub mod mounts;
pub mod probe;

#[derive(Debug, Error)]
pub enum Error {
    #[error("from io: {0}")]
    Io(#[from] std::io::Error),

    #[error("c stdlib: {0}")]
    StdLib(#[from] nix::Error),

    #[error("no such mount: {0}")]
    UnknownMount(PathBuf),

    #[error("no such device: {0}")]
    InvalidDevice(PathBuf),

    #[error("failed to read superblock: {0}")]
    Superblock(#[from] superblock::Error),

    #[error("superblock contains invalid unicode: {0}")]
    SuperblockUnicode(#[from] superblock::UnicodeError),
}
