// SPDX-FileCopyrightText: Copyright © 2025 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! Processing of `/proc/self/mounts` & `/etc/mtab`

use std::{io, path::Path};

use fs_err as fs;

/// Encapsulates a `/proc/self/mounts` or mtab file, ignoring fstab specific 5&6 columns
#[derive(Debug)]
pub struct Mount<'a> {
    /// Path of device used for mounting
    pub device: &'a str,

    /// Where the device was mounted
    pub mountpoint: &'a str,

    /// The filesystem name
    pub filesystem: &'a str,

    /// Raw mount options
    pub opts: &'a str,
}

/// Filesystem specific mount option, i.e `subvol=root`
#[derive(Debug)]
pub enum MountOption<'a> {
    /// Simple mount flag
    Flag(&'a str),

    /// Key-value option for a mount
    Option(&'a str, &'a str),
}

impl MountOption<'_> {
    /// Returns true if this is a flag
    pub fn is_flag(&self) -> bool {
        match &self {
            MountOption::Flag(_) => true,
            MountOption::Option(_, _) => false,
        }
    }

    /// Returns true if this is a key=value mapping
    pub fn is_option(&self) -> bool {
        !self.is_flag()
    }
}

impl Mount<'_> {
    /// Convert [`Mount::opts`] into an iterator of typed options
    pub fn options(&self) -> impl Iterator<Item = MountOption<'_>> {
        self.opts.split(',').map(|o| {
            if let Some((k, v)) = o.split_once('=') {
                MountOption::Option(k, v)
            } else {
                MountOption::Flag(o)
            }
        })
    }
}

/// MountTable for iterating mount points
#[derive(Debug, Default)]
pub struct Table {
    data: String,
}

impl Table {
    /// New MountTable parser for string
    ///
    /// Arguments:
    ///
    /// `data` - Some owned string
    pub fn new(data: String) -> Self {
        Self { data }
    }

    /// Iterate all mount points (no copy)
    pub fn iter(&self) -> impl Iterator<Item = Mount<'_>> {
        self.data.lines().filter_map(|i| {
            let mut splits = i.split_ascii_whitespace();
            Some(Mount {
                device: splits.next()?,
                mountpoint: splits.next()?,
                filesystem: splits.next()?,
                opts: splits.next()?,
            })
        })
    }

    /// New MountTable parser for file
    ///
    /// Arguments:
    ///
    /// `path` - Path to load the mtab from (i.e. `/proc/self/mounts`)
    pub fn new_from_path(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        Ok(Self::new(fs::read_to_string(path)?))
    }
}
