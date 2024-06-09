// SPDX-FileCopyrightText: Copyright © 2024 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! Boot environment tracking (ESP vs XBOOTLDR, etc)

use std::{
    fs::{self, File},
    path::PathBuf,
};

use gpt::{partition_types, GptConfig};
use topology::disk::probe::Probe;

use crate::{
    bootloader::systemd_boot::interface::{BootLoaderInterface, VariableName},
    Configuration, Error,
};

/// Type of firmware detected
///
/// By knowing the available firmware (effectively: is `efivarfs` mounted)
/// we can detect full availability of UEFI features or legacy fallback.
#[derive(Debug)]
pub enum Firmware {
    /// UEFI
    UEFI,

    /// Legacy BIOS. Tread carefully
    BIOS,
}

/// Helps access the boot environment, ie `$BOOT` and specific ESP
#[derive(Debug)]
pub struct BootEnvironment {
    /// xbootldr device
    xbootldr: Option<PathBuf>,

    /// The EFI System Partition (stored as a device path)
    esp: Option<PathBuf>,
    firmware: Firmware,
}

impl BootEnvironment {
    /// Return a new BootEnvironment for the given root
    pub fn new(probe: &Probe, disk_parent: Option<PathBuf>, config: &Configuration) -> Result<Self, Error> {
        let firmware = if config.vfs.join("sys").join("firmware").join("efi").exists() {
            Firmware::UEFI
        } else {
            Firmware::BIOS
        };

        // Layered discovery for ESP
        let esp = if let Ok(device) = Self::determine_esp_by_bls(&firmware, config) {
            Some(device)
        } else if let Ok(device) = Self::determine_esp_by_gpt(disk_parent, config) {
            Some(device)
        } else {
            None
        };

        // Make sure our config is sane!
        if let Firmware::UEFI = firmware {
            if esp.is_none() {
                log::error!("No usable ESP detected for a UEFI system");
                return Err(Error::NoESP);
            }
        }

        // Report ESP and check for XBOOTLDR
        if let Some(esp_path) = esp.as_ref() {
            log::info!("EFI System Partition: {}", esp_path.display());
            let xbootldr = if let Ok(xbootldr) = Self::discover_xbootldr(probe, esp_path, config) {
                log::info!("EFI XBOOTLDR Partition: {}", xbootldr.display());
                Some(xbootldr)
            } else {
                None
            };

            Ok(Self {
                xbootldr,
                esp,
                firmware,
            })
        } else {
            Ok(Self {
                xbootldr: None,
                esp,
                firmware,
            })
        }
    }

    /// If UEFI we can ask BootLoaderProtocol for help to find out the ESP device.
    fn determine_esp_by_bls(firmware: &Firmware, config: &Configuration) -> Result<PathBuf, Error> {
        // UEFI only tyvm
        if let Firmware::BIOS = *firmware {
            return Err(Error::Unsupported);
        }

        let systemd = BootLoaderInterface::new(&config.vfs)?;
        let info = systemd.get_ucs2_string(VariableName::Info)?;
        log::trace!("Encountered BLS compatible bootloader: {info}");
        Ok(systemd.get_device_path()?)
    }

    /// Determine ESP by searching relative GPT
    fn determine_esp_by_gpt(disk_parent: Option<PathBuf>, config: &Configuration) -> Result<PathBuf, Error> {
        let parent = disk_parent.ok_or(Error::Unsupported)?;
        log::trace!("Finding ESP on device: {:?}", parent);
        let device = Box::new(File::open(&parent)?);
        let table = GptConfig::new()
            .initialized(true)
            .writable(false)
            .open_from_device(device)?;
        let (_, esp) = table
            .partitions()
            .iter()
            .find(|(_, p)| p.part_type_guid == partition_types::EFI)
            .ok_or(Error::NoESP)?;
        let path = config
            .vfs
            .join("dev")
            .join("disk")
            .join("by-partuuid")
            .join(esp.part_guid.as_hyphenated().to_string());
        Ok(fs::canonicalize(path)?)
    }

    /// Discover an XBOOTLDR partition *relative* to wherever the ESP is
    fn discover_xbootldr(probe: &Probe, esp: &PathBuf, config: &Configuration) -> Result<PathBuf, Error> {
        let parent = probe.get_device_parent(esp).ok_or(Error::Unsupported)?;
        log::trace!("Finding XBOOTLDR on device: {:?}", parent);
        let device = Box::new(File::open(&parent)?);
        let table = GptConfig::new()
            .initialized(true)
            .writable(false)
            .open_from_device(device)?;
        let (_, esp) = table
            .partitions()
            .iter()
            .find(|(_, p)| p.part_type_guid == partition_types::FREEDESK_BOOT)
            .ok_or(Error::NoXBOOTLDR)?;
        let path = config
            .vfs
            .join("dev")
            .join("disk")
            .join("by-partuuid")
            .join(esp.part_guid.as_hyphenated().to_string());
        Ok(fs::canonicalize(path)?)
    }

    /// The so-called `$BOOT` partition (UEFI only at present)
    pub fn boot_partition(&self) -> Option<&PathBuf> {
        if let Some(part) = self.xbootldr.as_ref() {
            Some(part)
        } else {
            self.esp.as_ref()
        }
    }

    /// Return the EFI System Partition (UEFI only)
    pub fn esp(&self) -> Option<&PathBuf> {
        self.esp.as_ref()
    }
}
