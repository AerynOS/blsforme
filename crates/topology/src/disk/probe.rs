// SPDX-FileCopyrightText: Copyright © 2024 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! Disk probe/query APIs

use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use nix::sys::stat;
use superblock::Superblock;

use super::{device::BlockDevice, mounts::Table};

/// A Disk probe to query disks
#[derive(Debug)]
pub struct Probe {
    /// location of /sys
    pub(super) sysfs: PathBuf,

    /// location of /dev
    pub(super) devfs: PathBuf,

    /// location of /proc
    pub(super) procfs: PathBuf,

    /// Mountpoints
    pub mounts: Table,
}

impl Probe {
    /// Initial startup loads
    /// TODO: If requested, pvscan/vgscan/lvscan
    pub(super) fn init_scan(&mut self) -> Result<(), super::Error> {
        let mounts = Table::new_from_path(self.procfs.join("self").join("mounts"))?;
        self.mounts = mounts;

        Ok(())
    }

    /// Resolve a device by mountpoint
    pub fn get_device_from_mountpoint(&self, mountpoint: impl AsRef<Path>) -> Result<PathBuf, super::Error> {
        let mountpoint = fs::canonicalize(mountpoint.as_ref())?;

        // Attempt to stat the device
        let stat = stat::lstat(&mountpoint)?;
        let device_path =
            self.devfs
                .join("block")
                .join(format!("{}:{}", stat::major(stat.st_dev), stat::minor(stat.st_dev)));

        // Return by stat path if possible, otherwise fallback to mountpoint device
        if device_path.exists() {
            Ok(fs::canonicalize(&device_path)?)
        } else {
            // Find matching mountpoint
            let matching_device = self
                .mounts
                .iter()
                .find(|m| PathBuf::from(m.mountpoint) == mountpoint)
                .ok_or_else(|| super::Error::UnknownMount(mountpoint))?;
            // TODO: Handle `ZFS=`, and composite bcachefs mounts (dev:dev1:dev2)
            Ok(matching_device.device.into())
        }
    }

    /// Retrieve the parent device, such as the disk of a partition, if possible
    pub fn get_device_parent(&self, device: impl AsRef<Path>) -> Option<PathBuf> {
        let device = fs::canonicalize(device.as_ref()).ok()?;
        let child = fs::canonicalize(
            device
                .file_name()
                .map(|f| self.sysfs.join("class").join("block").join(f))?,
        )
        .ok()?;
        let parent = child.parent()?.file_name()?;
        if parent == "block" {
            None
        } else {
            fs::canonicalize(self.devfs.join(parent)).ok()
        }
    }

    /// When given a path in `/dev` we attempt to resolve the full chain for it.
    /// Note: This does NOT include the initially passed device.
    pub fn get_device_chain(&self, device: impl AsRef<Path>) -> Result<Vec<PathBuf>, super::Error> {
        let device = fs::canonicalize(device.as_ref())?;
        let sysfs_path = fs::canonicalize(
            device
                .file_name()
                .map(|f| self.sysfs.join("class").join("block").join(f))
                .ok_or_else(|| super::Error::InvalidDevice(device.clone()))?,
        )?;

        let mut ret = vec![];
        // no backing devices
        let dir = sysfs_path.join("slaves");
        if !dir.exists() {
            return Ok(ret);
        }

        // Build a recursive set of device backings
        for dir in fs::read_dir(dir)? {
            let entry = dir?;
            let name = self.devfs.join(entry.file_name());
            ret.push(name.clone());
            ret.extend(self.get_device_chain(&name)?);
        }

        Ok(ret)
    }

    /// Scan superblock of the device for `UUID=` parameter
    pub fn get_device_superblock(&self, path: impl AsRef<Path>) -> Result<Box<dyn Superblock>, super::Error> {
        let path = path.as_ref();
        log::trace!("Querying superblock information for {}", path.display());
        let fi = fs::File::open(path)?;
        let mut buffer: Vec<u8> = Vec::with_capacity(2 * 1024 * 1024);
        fi.take(2 * 1024 * 1024).read_to_end(&mut buffer)?;
        let mut cursor = Cursor::new(&buffer);
        let sb = superblock::for_reader(&mut cursor)?;
        log::trace!("detected superblock: {}", sb.kind());

        Ok(sb)
    }

    /// Determine the composite rootfs device for the given mountpoint,
    /// building a set of superblocks and necessary `/proc/cmdline` arguments
    pub fn get_rootfs_device(&self, path: impl AsRef<Path>) -> Result<BlockDevice, super::Error> {
        let path = path.as_ref();
        let device = self.get_device_from_mountpoint(path)?;

        // Scan GPT for PartUUID
        let guid = if let Some(parent) = self.get_device_parent(&device) {
            self.get_device_guid(parent, &device)
        } else {
            None
        };

        let chain = self.get_device_chain(&device)?;
        let mut custodials = vec![device.clone()];
        custodials.extend(chain);

        let tip = custodials.pop().expect("we just added this..");
        let name = tip.to_string_lossy().to_string();

        let mut block = BlockDevice::new(self, &name, None, true)?;
        block.children = custodials
            .iter()
            .flat_map(|c| {
                if *c == device {
                    BlockDevice::new(self, c.clone(), Some(path.into()), false)
                } else {
                    BlockDevice::new(self, c.clone(), None, true)
                }
            })
            .collect::<Vec<_>>();
        block.guid = guid;

        Ok(block)
    }

    /// For GPT disks return the PartUUID (GUID)
    pub fn get_device_guid(&self, parent: impl AsRef<Path>, path: impl AsRef<Path>) -> Option<String> {
        let device = fs::canonicalize(path.as_ref()).ok()?;
        let sysfs_path = fs::canonicalize(
            device
                .file_name()
                .map(|f| self.sysfs.join("class").join("block").join(f))
                .ok_or_else(|| super::Error::InvalidDevice(device.clone()))
                .ok()?,
        )
        .ok()?;
        let partition = str::parse::<u32>(fs::read_to_string(sysfs_path.join("partition")).ok()?.trim()).ok()?;
        let fi = fs::File::open(parent).ok()?;
        let gpt_header = gpt::GptConfig::new()
            .writable(false)
            .initialized(true)
            .open_from_device(Box::new(fi))
            .ok()?;
        gpt_header
            .partitions()
            .get(&partition)
            .map(|partition| partition.part_guid.hyphenated().to_string())
    }
}
