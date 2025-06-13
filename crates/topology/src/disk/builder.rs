// SPDX-FileCopyrightText: Copyright © 2025 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! Builder API for constructing the Probe
use std::path::PathBuf;

use fs_err as fs;
use snafu::ResultExt as _;

use super::{mounts::Table, CanonicalizeSnafu};
use crate::disk::probe::Probe;

/// Builder pattern for a Probe
pub struct Builder {
    sysfs: PathBuf,
    devfs: PathBuf,
    procfs: PathBuf,
}

impl Default for Builder {
    fn default() -> Self {
        Builder {
            sysfs: "/sys".into(),
            devfs: "/dev".into(),
            procfs: "/proc".into(),
        }
    }
}

impl Builder {
    // sysfs directory
    pub fn with_sysfs(self, sysfs: impl Into<PathBuf>) -> Self {
        Self {
            sysfs: sysfs.into(),
            ..self
        }
    }

    /// devfs directory
    pub fn with_devfs(self, devfs: impl Into<PathBuf>) -> Self {
        Self {
            devfs: devfs.into(),
            ..self
        }
    }

    // procfs directory
    pub fn with_procfs(self, procfs: impl Into<PathBuf>) -> Self {
        Self {
            procfs: procfs.into(),
            ..self
        }
    }

    /// Return a newly built Probe
    /// Note: All input paths will be verified
    pub fn build(self) -> Result<Probe, super::Error> {
        let mut result = Probe {
            sysfs: fs::canonicalize(self.sysfs).context(CanonicalizeSnafu)?,
            devfs: fs::canonicalize(self.devfs).context(CanonicalizeSnafu)?,
            procfs: fs::canonicalize(self.procfs).context(CanonicalizeSnafu)?,
            mounts: Table::default(),
        };
        result.init_scan()?;
        Ok(result)
    }
}
