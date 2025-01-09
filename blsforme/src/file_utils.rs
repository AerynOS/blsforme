// SPDX-FileCopyrightText: Copyright © 2025 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! File utilities shared between the blsforme APIs

use std::{
    fs::{self, create_dir_all, File},
    io,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
};

use crate::Error;

/// Case-insensitive path joining for FAT, respecting existing entries on the filesystem
/// Note, this discards errors, so will require read permissions
pub trait PathExt<P: AsRef<Path>> {
    fn join_insensitive(&self, path: P) -> PathBuf;
}

impl<P: AsRef<Path>> PathExt<P> for PathBuf {
    fn join_insensitive(&self, path: P) -> PathBuf {
        let real_path: &Path = path.as_ref();
        if let Ok(dir) = fs::read_dir(self) {
            let entries = dir.filter_map(|e| e.ok()).filter_map(|p| {
                let n = p.file_name();
                n.into_string().ok()
            });
            for entry in entries {
                if entry.to_lowercase() == real_path.to_string_lossy().to_lowercase() {
                    return self.join(&entry);
                }
            }
        }
        self.join(path)
    }
}

/// Compare two files with blake3 to see if they differ
fn files_identical(hasher: &mut blake3::Hasher, a: &Path, b: &Path) -> Result<bool, Error> {
    let fi_a = File::open(a)?;
    let fi_b = File::open(b)?;
    let fi_a_m = fi_a.metadata()?;
    let fi_b_m = fi_b.metadata()?;
    if fi_a_m.size() != fi_b_m.size() || fi_a_m.file_type() != fi_b_m.file_type() {
        Ok(false)
    } else {
        hasher.update_mmap_rayon(a)?;
        let result_a = hasher.finalize();
        hasher.reset();

        hasher.update_mmap_rayon(b)?;
        let result_b = hasher.finalize();
        hasher.reset();

        Ok(result_a == result_b)
    }
}

/// Find out which files in the set changed
///
/// Given a slice containing tuples of pathbufs, return an
/// allocated set of cloned pathbuf tuples (pairs) known to
/// differ.
///
/// The first element in the tuple should be the source path, and the
/// right hand side should contain the destination path.
pub fn changed_files(files: &[(PathBuf, PathBuf)]) -> Vec<(&PathBuf, &PathBuf)> {
    let mut hasher = blake3::Hasher::new();

    files
        .iter()
        .filter_map(|(source, dest)| match files_identical(&mut hasher, source, dest) {
            Ok(same) => {
                if same {
                    None
                } else {
                    Some((source, dest))
                }
            }
            Err(_) => Some((source, dest)),
        })
        .collect::<Vec<_>>()
}

/// Copy source file to dest file, handling vfat oddities.
///
/// Long story short we always set a temporary file name up,
/// then delete the target file, and finally rename into place.
/// This is to prevent various block corruption issues with vfat.
pub fn copy_atomic_vfat(
    source: impl AsRef<Path>,
    dest: impl AsRef<Path>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let source = source.as_ref();
    let dest = dest.as_ref();

    log::trace!("copy_atomic_vfat: {}", dest.display());

    // Staging path
    let dest_temp = dest.with_extension(".TmpWrite");
    let dest_exists = dest.exists();

    // Ensure leading path structure exists
    let dir_leading = dest.parent().ok_or_else(|| Error::InvalidFilesystem)?;
    if !dir_leading.exists() {
        create_dir_all(dir_leading)?;
    }

    // open source/dest
    let mut output = File::options()
        .truncate(true)
        .write(true)
        .create(true)
        .open(&dest_temp)?;
    let mut input = File::open(source)?;

    let output_fd = output.as_raw_fd();

    // Copy *contents* only
    io::copy(&mut input, &mut output)?;
    nix::unistd::syncfs(output_fd)?;

    // Remove original destination file
    if dest_exists {
        fs::remove_file(dest)?;
        nix::unistd::syncfs(output_fd)?;
    }

    // Rename into final location
    fs::rename(dest_temp, dest)?;
    nix::unistd::syncfs(output_fd)?;

    log::info!("Updated VFAT file: {}", dest.display());

    Ok(())
}

/// Read a cmdline snippet from a file, which supports comments (`#`)
/// and concatenates lines into a single string.
pub fn cmdline_snippet(path: impl AsRef<Path>) -> Result<String, Error> {
    let path = path.as_ref();
    log::trace!("Reading cmdline snippet: {path:?}");
    let ret = fs::read_to_string(path)?
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ")
        .to_string();
    Ok(ret)
}
