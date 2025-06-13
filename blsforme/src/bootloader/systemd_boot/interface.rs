// SPDX-FileCopyrightText: Copyright © 2025 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! Boot Loader Interface (systemd)
//!
//! Access helpers for interacting with the [Boot Loader Interface](https://systemd.io/BOOT_LOADER_INTERFACE/)
//! by way of EFI variables accessed via the `efivars` mount.
//!

use std::{
    fmt::Display,
    fs, io,
    path::{self, Path, PathBuf},
    string::FromUtf16Error,
};

use thiserror::Error;

/// Simple encapsulation of a Boot Loader Interface over efivars
pub struct BootLoaderInterface {
    /// All queries are performed relative to this root to permit mocking
    root: PathBuf,

    /// EFI vars directory relative to root
    efi_dir: PathBuf,

    /// The /dev/disk/by-partuuid dir
    disk_dir: PathBuf,
}

/// The well known vendor UUID for the Boot Loader Interface
pub const UUID: &str = "4a67b082-0a4c-41cf-b6c7-440b29bb8c4f";

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to decode UTF16 string: {0}")]
    Utf16Decoding(#[from] FromUtf16Error),

    #[error("i/o error: {0}")]
    Io(#[from] io::Error),

    #[error("malformed utf16 string")]
    Malformed,

    #[error("invalid prefix: {0}")]
    InvalidPrefix(#[from] path::StripPrefixError),
}

/// Variables that are currently exposed via efivars
pub enum VariableName {
    TimeInitUSec,
    TimeExecUSec,
    DevicePartUuid,
    ConfigTimeout,
    ConfigTimeoutOneShot,
    Entries,
    EntryDefault,
    EntrySelected,
    Features,
    ImageIdentifier,
    Info,
    SystemToken,
}

impl VariableName {
    /// Convert the variable into a static string representation for the EFI variable name
    fn as_str(&self) -> &'static str {
        match self {
            VariableName::TimeInitUSec => "LoaderTimeInitUSec",
            VariableName::TimeExecUSec => "LoaderTimeExecUSec",
            VariableName::DevicePartUuid => "LoaderDevicePartUUID",
            VariableName::ConfigTimeout => "LoaderConfigTimeout",
            VariableName::ConfigTimeoutOneShot => "LoaderConfigTimeoutOneShot",
            VariableName::Entries => "LoaderEntries",
            VariableName::EntryDefault => "LoaderEntryDefault",
            VariableName::EntrySelected => "LoaderEntrySelected",
            VariableName::Features => "LoaderFeatures",
            VariableName::ImageIdentifier => "LoaderImageIdentifier",
            VariableName::Info => "LoaderInfo",
            VariableName::SystemToken => "LoaderSystemToken",
        }
    }
}

impl Display for VariableName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl BootLoaderInterface {
    /// Generate a new BootLoaderInterface for the given root
    pub fn new(root: impl AsRef<Path>) -> Result<Self, Error> {
        let root: PathBuf = fs::canonicalize(root)?;
        let efi_dir = root.join("sys").join("firmware").join("efi").join("efivars");
        let disk_dir = root.join("dev").join("disk").join("by-partuuid");

        Ok(Self {
            root,
            efi_dir,
            disk_dir,
        })
    }

    /// Grab the PartUUID for the ESP-booting device
    pub fn get_device_part_uuid(&self) -> Result<String, Error> {
        Ok(self.get_ucs2_string(VariableName::DevicePartUuid)?.to_lowercase())
    }

    /// Determine which device "booted", ie the ESP on which systemd-boot lives
    pub fn get_device_path(&self) -> Result<PathBuf, Error> {
        let canonical = fs::canonicalize(self.disk_dir.join(self.get_device_part_uuid()?))?;
        Ok(PathBuf::from("/").join(canonical.strip_prefix(&self.root)?))
    }

    /// Grab a UCS2 string from efivars
    pub fn get_ucs2_string(&self, var: VariableName) -> Result<String, Error> {
        let mut raw = fs::read(self.join_var(var))?
            .chunks(2)
            .skip(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>();
        raw.pop();
        Ok(String::from_utf16(&raw)?)
    }

    /// Generate root path for the variable
    fn join_var(&self, var: VariableName) -> PathBuf {
        self.efi_dir.join(format!("{var}-{UUID}"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::BootLoaderInterface;

    #[test]
    fn basic_interface_test() {
        let b = BootLoaderInterface::new("../test").expect("Failed to create BLI");
        let dev = b.get_device_path().expect("Unable to fetch DevicePartUUID");
        assert_eq!(dev, PathBuf::from("/dev/nvme0n1p1"));
    }
}
