// SPDX-FileCopyrightText: Copyright © 2025 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

use std::path::PathBuf;

use crate::{file_utils::cmdline_snippet, AuxiliaryFile, Configuration, Kernel, Schema};

/// A cmdline entry is found in the `$sysroot/usr/lib/kernel/cmdline.d` directory
#[derive(Debug)]
pub struct CmdlineEntry {
    /// Name of the entry, i.e. `00-quiet.cmdline`
    pub name: String,

    /// Text contents of this cmdline entry
    pub snippet: String,
}

/// An entry corresponds to a single kernel, and may have a supplemental
/// cmdline
#[derive(Debug)]
pub struct Entry<'a> {
    pub(crate) kernel: &'a Kernel,

    pub(crate) sysroot: Option<PathBuf>,

    pub(crate) cmdline: Vec<CmdlineEntry>,

    /// Unique state ID for this entry
    pub(crate) state_id: Option<i32>,
}

impl<'a> Entry<'a> {
    /// New entry for the given kernel
    pub fn new(kernel: &'a Kernel) -> Self {
        Self {
            kernel,
            cmdline: vec![],
            sysroot: None,
            state_id: None,
        }
    }

    /// Load cmdline snippets from the system root for this entry's sysroot
    pub fn load_cmdline_snippets(&mut self, config: &Configuration) -> Result<(), super::Error> {
        let sysroot = self.sysroot.clone().unwrap_or(config.root.path().into());

        // Load local cmdline snippets for this kernel entry
        for snippet in self
            .kernel
            .extras
            .iter()
            .filter(|e| matches!(e.kind, crate::AuxiliaryKind::Cmdline))
        {
            if let Ok(cmdline) = cmdline_snippet(sysroot.join(&snippet.path)) {
                self.cmdline.push(CmdlineEntry {
                    name: snippet.path.file_name().unwrap().to_string_lossy().to_string(),
                    snippet: cmdline,
                });
            }
        }

        // Globals
        let cmdline_d = sysroot.join("usr").join("lib").join("kernel").join("cmdline.d");

        if !cmdline_d.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(&cmdline_d)?;

        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().to_string();
            // Don't bomb out on invalid cmdline snippets
            if let Ok(snippet) = cmdline_snippet(entry.path()) {
                self.cmdline.push(CmdlineEntry { name, snippet });
            }
        }

        Ok(())
    }

    /// With the given system root
    /// This will cause any local snippets to be discovered
    pub fn with_sysroot(self, sysroot: impl Into<PathBuf>) -> Self {
        Self {
            sysroot: Some(sysroot.into()),
            ..self
        }
    }

    /// With the given state ID
    /// Used by moss to link to the unique transaction ID on disk
    pub fn with_state_id(self, state_id: i32) -> Self {
        Self {
            state_id: Some(state_id),
            ..self
        }
    }

    /// With the given cmdline entry
    /// Used by moss to inject a `moss.tx={}` parameter
    pub fn with_cmdline(self, entry: CmdlineEntry) -> Self {
        let mut cmdline = self.cmdline;
        cmdline.push(entry);
        Self { cmdline, ..self }
    }

    /// Return an entry ID, suitable for `.conf` generation
    pub fn id(&self, schema: &Schema) -> String {
        // TODO: For BLS schema, grab something even uniquer (TM)
        let id = match schema {
            Schema::Legacy { os_release, .. } => os_release.name.clone(),
            Schema::Blsforme { os_release } => os_release.id.clone(),
        };
        if let Some(state_id) = self.state_id.as_ref() {
            format!("{id}-{version}-{state_id}", version = &self.kernel.version)
        } else {
            format!("{id}-{version}", version = &self.kernel.version)
        }
    }

    /// Generate an installed name for the kernel, used by bootloaders
    /// Right now this only returns CBM style IDs
    pub fn installed_kernel_name(&self, schema: &Schema) -> Option<String> {
        match &schema {
            Schema::Legacy { .. } => self
                .kernel
                .image
                .file_name()
                .map(|f| f.to_string_lossy())
                .map(|filename| format!("kernel-{}", filename)),
            Schema::Blsforme { .. } => Some(format!("{}/vmlinuz", self.kernel.version)),
        }
    }

    /// Generate installed asset (aux) name, used by bootloaders
    /// Right now this only returns CBM style IDs
    pub fn installed_asset_name(&self, schema: &Schema, asset: &AuxiliaryFile) -> Option<String> {
        match &schema {
            Schema::Legacy { .. } => match asset.kind {
                crate::AuxiliaryKind::InitRD => asset
                    .path
                    .file_name()
                    .map(|f| f.to_string_lossy())
                    .map(|filename| format!("initrd-{}", filename)),
                _ => None,
            },
            Schema::Blsforme { .. } => {
                let filename = asset.path.file_name().map(|f| f.to_string_lossy())?;
                match asset.kind {
                    crate::AuxiliaryKind::InitRD => Some(format!("{}/{}", &self.kernel.version, filename)),
                    _ => None,
                }
            }
        }
    }
}
