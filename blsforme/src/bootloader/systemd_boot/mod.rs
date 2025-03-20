// SPDX-FileCopyrightText: Copyright © 2025 Serpent OS Developers
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

    schema: &'a Schema,
    boot_root: PathBuf,
}

#[derive(Debug)]
struct InstallResult {
    /// The `.conf` file that was written (absolute)
    loader_conf: String,

    // The kernel path that was installed (absolute)
    kernel_dir: String,
}

impl<'a, 'b> Loader<'a, 'b> {
    /// Construct a new systemd boot loader manager
    pub(super) fn new(schema: &'a Schema, assets: &'b [PathBuf], mounts: &'a Mounts) -> Result<Self, super::Error> {
        let boot_root = if let Some(xbootldr) = mounts.xbootldr.as_ref() {
            xbootldr.clone()
        } else if let Some(esp) = mounts.esp.as_ref() {
            esp.clone()
        } else {
            return Err(super::Error::MissingMount("ESP (/efi)"));
        };

        Ok(Self {
            schema,
            assets,
            mounts,
            boot_root,
        })
    }

    /// Get the kernel directory for a specific entry
    fn get_kernel_dir(&self, entry: &Entry) -> PathBuf {
        let effective_schema = entry.schema.as_ref().unwrap_or(self.schema);
        self.boot_root
            .join_insensitive("EFI")
            .join_insensitive(effective_schema.os_namespace())
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

    pub(super) fn sync_entries(
        &self,
        cmdline: impl Iterator<Item = &'a str>,
        entries: &[Entry],
        excluded_snippets: impl Iterator<Item = &'a str>,
    ) -> Result<(), super::Error> {
        let base_cmdline = cmdline.map(str::to_string).collect::<Vec<_>>();
        let exclusions = excluded_snippets.map(str::to_string).collect::<Vec<_>>();
        let mut installed_entries = vec![];
        for entry in entries {
            let entry_cmdline = entry
                .cmdline
                .iter()
                .filter(|c| !exclusions.contains(&c.name))
                .map(|c| c.snippet.clone())
                .collect::<Vec<_>>();
            let full_cmdline = base_cmdline
                .iter()
                .chain(entry_cmdline.iter())
                .cloned()
                .collect::<Vec<_>>();

            let installed = self.install(&full_cmdline.join(" "), entry)?;
            installed_entries.push(installed);
        }

        let schema_prefix = match self.schema {
            Schema::Legacy { os_release, .. } => os_release.name.clone(),
            _ => self.schema.os_id(),
        };

        let loader_dir = self.boot_root.join_insensitive("loader").join_insensitive("entries");
        let loader_files = fs::read_dir(loader_dir)?
            .filter_map(|d| d.ok())
            .filter(|f| f.file_name().to_string_lossy().to_string().starts_with(&schema_prefix))
            .map(|f| f.path())
            .collect::<Vec<_>>();

        let base_kernel_dir = self
            .boot_root
            .join_insensitive("EFI")
            .join_insensitive(self.schema.os_namespace());

        let kernel_dirs = fs::read_dir(&base_kernel_dir)?
            .filter_map(|d| d.ok())
            .filter(|f| f.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|f| f.path())
            .collect::<Vec<_>>();

        let obsolete_loader_confs = loader_files
            .iter()
            .filter(|f| !installed_entries.iter().any(|e| e.loader_conf == f.to_string_lossy()))
            .collect::<Vec<_>>();

        let obsolete_kernels = kernel_dirs
            .iter()
            .filter(|f| !installed_entries.iter().any(|e| e.kernel_dir == f.to_string_lossy()))
            .collect::<Vec<_>>();

        for conf in obsolete_loader_confs.iter() {
            log::info!("Removing stale loader config: {conf:?}");
            if let Err(e) = fs::remove_file(conf) {
                log::error!("Failed to remove stale loader config {conf:?}: {e}")
            }
        }

        for tree in obsolete_kernels.iter() {
            log::info!("Removing stale kernel tree: {tree:?}");
            if let Err(e) = fs::remove_dir_all(tree) {
                log::error!("Failed to remove stale kernel tree {tree:?}: {e}")
            }
        }

        Ok(())
    }

    /// Install a kernel to the ESP or XBOOTLDR, write a config for it
    fn install(&self, cmdline: &str, entry: &Entry) -> Result<InstallResult, super::Error> {
        let effective_schema = entry.schema.as_ref().unwrap_or(self.schema);

        let loader_id = self
            .boot_root
            .join_insensitive("loader")
            .join_insensitive("entries")
            .join_insensitive(format!("{}.conf", entry.id(effective_schema)));
        log::trace!("writing entry: {}", loader_id.display());

        let sysroot = entry.sysroot.clone().unwrap_or_default();

        // Get kernel directory for this specific entry
        let kernel_dir = self.get_kernel_dir(entry);

        // vmlinuz primary path
        let vmlinuz = kernel_dir.join_insensitive(
            entry
                .installed_kernel_name(effective_schema)
                .ok_or_else(|| super::Error::MissingFile("vmlinuz"))?,
        );
        // initrds requiring install
        let initrds = entry
            .kernel
            .initrd
            .iter()
            .filter_map(|asset| {
                Some((
                    sysroot.join(&asset.path),
                    kernel_dir.join_insensitive(entry.installed_asset_name(effective_schema, asset)?),
                ))
            })
            .collect::<Vec<_>>();
        log::trace!("with kernel path: {}", vmlinuz.display());
        log::trace!("with initrds: {:?}", initrds);

        // build up the total changeset
        let mut changeset = vec![(sysroot.join(&entry.kernel.image), vmlinuz.clone())];
        changeset.extend(initrds);

        // Determine which need copying now.
        let needs_writing = changed_files(changeset.as_slice());
        log::trace!("requires update: {needs_writing:?}");

        // Donate them to disk
        for (source, dest) in needs_writing {
            copy_atomic_vfat(source, dest)?;
        }

        let asset_dir = kernel_dir.strip_prefix(&self.boot_root)?.to_string_lossy();

        let loader_config = self.generate_entry(&asset_dir, cmdline, entry);
        log::trace!("loader config: {loader_config}");

        let entry_dir = self.boot_root.join_insensitive("loader").join_insensitive("entries");
        if !entry_dir.exists() {
            create_dir_all(entry_dir)?;
        }

        let tracker = InstallResult {
            loader_conf: loader_id.to_string_lossy().to_string(),
            kernel_dir: vmlinuz
                .parent()
                .ok_or_else(|| super::Error::MissingFile("vmlinuz parent"))?
                .to_string_lossy()
                .to_string(),
        };

        // TODO: Hash compare and dont obliterate!
        fs::write(loader_id, loader_config)?;

        Ok(tracker)
    }

    /// Generate a usable loader config entry
    fn generate_entry(&self, asset_dir: &str, cmdline: &str, entry: &Entry) -> String {
        let effective_schema = entry.schema.as_ref().unwrap_or(self.schema);

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
                        entry.installed_asset_name(effective_schema, asset)?
                    ))
                })
                .collect::<String>();
            format!("\n{}", initrds)
        };
        let title = if let Some(pretty) = effective_schema.os_display_name() {
            format!("{pretty} ({})", entry.kernel.version)
        } else {
            format!("{} ({})", effective_schema.os_name(), entry.kernel.version)
        };
        let vmlinuz = entry.installed_kernel_name(effective_schema).expect("linux go boom");
        format!(
            r###"title {title}
linux /{asset_dir}/{}{}
options {cmdline}
"###,
            vmlinuz, initrd
        )
    }

    pub fn installed_kernels(&self) -> Result<Vec<Kernel>, super::Error> {
        let mut all_paths = vec![];
        let base_kernel_dir = self
            .boot_root
            .join_insensitive("EFI")
            .join_insensitive(self.schema.os_namespace());

        for entry in fs::read_dir(&base_kernel_dir)? {
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
