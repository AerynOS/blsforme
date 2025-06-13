// SPDX-FileCopyrightText: Copyright © 2025 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! Disk probe/query APIs

use std::path::PathBuf;

use snafu::Snafu;

mod builder;
pub use builder::Builder;
pub mod device;
pub mod mounts;
pub mod probe;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("failed to canonicalize path: {source}"))]
    Canonicalize { source: std::io::Error },

    #[snafu(display("from io: {source}"))]
    Io { source: std::io::Error },

    #[snafu(display("c stdlib: {source}"))]
    Nix { source: nix::Error },

    #[snafu(display("no such mount: {path:?}"))]
    UnknownMount { path: PathBuf },

    #[snafu(display("no such device: {path:?}"))]
    InvalidDevice { path: PathBuf },

    #[snafu(context(false), display("failed to read superblock: {source}"))]
    Superblock { source: superblock::Error },

    #[snafu(context(false), display("superblock contains invalid unicode: {source}"))]
    SuperblockUnicode { source: superblock::UnicodeError },
}
