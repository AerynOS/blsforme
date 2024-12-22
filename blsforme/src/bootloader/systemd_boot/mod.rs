// SPDX-FileCopyrightText: Copyright © 2024 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! systemd-boot management and interfaces

use std::{
    fs::{self, create_dir_all},
    path::PathBuf,
};

use crate::{
    file_utils::{changed_files, copy_atomic_vfat, PathExt},
    manager::Mounts,
    Entry, Kernel, Schema,
};

pub mod interface;

/// systemd specific bootloader behaviours
/// NOTE: Currently secure boot is NOT supported (or fbx64)
#[derive(Debug)]
pub struct Loader<'a, 'b> {
    /// system configuration
    #[allow(dead_code)]
    assets: &'b [PathBuf],
    mounts: &'a Mounts,

    schema: &'a Schema<'a>,
    kernel_dir: PathBuf,
    boot_root: PathBuf,
}

impl<'a, 'b> Loader<'a, 'b> {
    /// Construct a new systemd boot loader manager
    pub(super) fn new(schema: &'a Schema<'a>, assets: &'b [PathBuf], mounts: &'a Mounts) -> Result<Self, super::Error> {
        let boot_root = if let Some(xbootldr) = mounts.xbootldr.as_ref() {
            xbootldr.clone()
        } else if let Some(esp) = mounts.esp.as_ref() {
            esp.clone()
        } else {
            return Err(super::Error::MissingMount("ESP (/efi)"));
        };

        let kernel_dir = match schema {
            Schema::Legacy { namespace, .. } => boot_root.join_insensitive("EFI").join_insensitive(namespace),
            Schema::Blsforme { os_release } => boot_root
                .join_insensitive("EFI")
                .join_insensitive(os_release.id.clone()),
        };

        Ok(Self {
            schema,
            assets,
            mounts,
            kernel_dir,
            boot_root,
        })
    }

    /// Sync bootloader to ESP (not XBOOTLDR..)
    pub(super) fn sync(&self) -> Result<(), super::Error> {
        let x64_efi = self
            .assets
            .iter()
            .find(|p| p.ends_with("systemd-bootx64.efi"))
            .ok_or(super::Error::MissingFile("systemd-bootx64.efi"))?;
        log::debug!("discovered main efi asset: {}", x64_efi.display());

        let esp = self
            .mounts
            .esp
            .as_ref()
            .ok_or(super::Error::MissingMount("ESP (/efi)"))?;
        // Copy systemd-bootx64.efi into these locations
        let targets = vec![
            (
                x64_efi.clone(),
                esp.join_insensitive("EFI")
                    .join_insensitive("Boot")
                    .join_insensitive("BOOTX64.EFI"),
            ),
            (
                x64_efi.clone(),
                esp.join_insensitive("EFI")
                    .join_insensitive("systemd")
                    .join_insensitive("systemd-bootx64.efi"),
            ),
        ];

        for (source, dest) in changed_files(targets.as_slice()) {
            copy_atomic_vfat(source, dest)?;
        }

        Ok(())
    }

    /// Install a kernel to the ESP or XBOOTLDR, write a config for it
    pub(super) fn install(&self, cmdline: &str, entry: &Entry) -> Result<(), super::Error> {
        let loader_id = self
            .boot_root
            .join_insensitive("loader")
            .join_insensitive("entries")
            .join_insensitive(entry.id(self.schema))
            .with_extension("conf");
        log::trace!("writing entry: {}", loader_id.display());

        // vmlinuz primary path
        let vmlinuz = self.kernel_dir.join_insensitive(
            entry
                .installed_kernel_name(self.schema)
                .ok_or_else(|| super::Error::MissingFile("vmlinuz"))?,
        );
        // initrds requiring install
        let initrds = entry
            .kernel
            .initrd
            .iter()
            .filter_map(|asset| {
                Some((
                    asset.path.clone(),
                    self.kernel_dir
                        .join_insensitive(entry.installed_asset_name(self.schema, asset)?),
                ))
            })
            .collect::<Vec<_>>();
        log::trace!("with kernel path: {}", vmlinuz.display());
        log::trace!("with initrds: {:?}", initrds);

        // build up the total changeset
        let mut changeset = vec![(entry.kernel.image.clone(), vmlinuz)];
        changeset.extend(initrds);

        // Determine which need copying now.
        let needs_writing = changed_files(changeset.as_slice());
        log::trace!("requires update: {needs_writing:?}");

        // Donate them to disk
        for (source, dest) in needs_writing {
            copy_atomic_vfat(source, dest)?;
        }

        let loader_config = self.generate_entry(
            self.kernel_dir
                .strip_prefix(&self.boot_root)?
                .to_string_lossy()
                .as_ref(),
            cmdline,
            entry,
        );
        log::trace!("loader config: {loader_config}");

        let entry_dir = self.boot_root.join_insensitive("loader").join_insensitive("entries");
        if !entry_dir.exists() {
            create_dir_all(entry_dir)?;
        }

        // TODO: Hash compare and dont obliterate!
        fs::write(loader_id, loader_config)?;

        Ok(())
    }

    /// Generate a usable loader config entry
    fn generate_entry(&self, asset_dir: &str, cmdline: &str, entry: &Entry) -> String {
        let initrd = if entry.kernel.initrd.is_empty() {
            "\n".to_string()
        } else {
            let initrds = entry
                .kernel
                .initrd
                .iter()
                .filter_map(|asset| {
                    Some(format!(
                        "\ninitrd /{asset_dir}/{}",
                        entry.installed_asset_name(self.schema, asset)?
                    ))
                })
                .collect::<String>();
            format!("\n{}", initrds)
        };
        let title = if let Some(pretty) = self.schema.os_release().meta.pretty_name.as_ref() {
            format!("{pretty} ({})", entry.kernel.version)
        } else {
            format!("{} ({})", self.schema.os_release().name, entry.kernel.version)
        };
        let vmlinuz = entry.installed_kernel_name(self.schema).expect("linux go boom");
        let options = if let Some(k_cmdline) = entry.kernel.cmdline.as_ref() {
            format!("{cmdline} {k_cmdline}")
        } else {
            cmdline.to_string()
        };
        format!(
            r###"title {title}
linux /{asset_dir}/{}{}
options {}
"###,
            vmlinuz, initrd, options
        )
    }

    pub fn installed_kernels(&self) -> Result<Vec<Kernel>, super::Error> {
        let mut all_paths = vec![];
        for entry in fs::read_dir(&self.kernel_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let paths = fs::read_dir(entry.path())?
                .filter_map(|p| p.ok())
                .map(|d| d.path())
                .collect::<Vec<_>>();
            all_paths.extend(paths);
        }

        if let Ok(kernels) = self.schema.discover_system_kernels(all_paths.iter()) {
            Ok(kernels)
        } else {
            Ok(vec![])
        }
    }
}
