// SPDX-FileCopyrightText: Copyright © 2025 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! Bootloader APIs

use std::path::{PathBuf, StripPrefixError};

use thiserror::Error;

use crate::{manager::Mounts, Entry, Firmware, Kernel, Schema};

pub mod systemd_boot;

/// Bootloader errors
#[derive(Error, Debug)]
pub enum Error {
    #[error("missing bootloader file: {0}")]
    MissingFile(&'static str),

    #[error("missing mountpoint: {0}")]
    MissingMount(&'static str),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("wip: {0}")]
    Prefix(#[from] StripPrefixError),
}

#[derive(Debug)]
pub enum Bootloader<'a, 'b> {
    /// We really only support systemd-boot right now
    Systemd(Box<systemd_boot::Loader<'a, 'b>>),
}

impl<'a, 'b> Bootloader<'a, 'b> {
    /// Construct the firmware-appropriate bootloader manager
    pub(crate) fn new(
        schema: &'a Schema,
        assets: &'b [PathBuf],
        mounts: &'a Mounts,
        firmware: &Firmware,
    ) -> Result<Self, Error> {
        match firmware {
            Firmware::Uefi => Ok(Bootloader::Systemd(Box::new(systemd_boot::Loader::new(
                schema, assets, mounts,
            )?))),
            Firmware::Bios => unimplemented!(),
        }
    }

    /// Sync bootloader to BOOT dir
    pub fn sync(&self) -> Result<(), Error> {
        match &self {
            Bootloader::Systemd(s) => s.sync(),
        }
    }

    pub fn sync_entries(
        &self,
        cmdline: impl Iterator<Item = &'a str>,
        entries: &[Entry],
        excluded_snippets: impl Iterator<Item = &'a str>,
    ) -> Result<(), Error> {
        match &self {
            Bootloader::Systemd(s) => s.sync_entries(cmdline, entries, excluded_snippets),
        }
    }

    /// Grab the installed entries
    pub fn installed_kernels(&self) -> Result<Vec<Kernel>, Error> {
        match &self {
            Bootloader::Systemd(s) => s.installed_kernels(),
        }
    }
}
