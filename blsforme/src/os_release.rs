// SPDX-FileCopyrightText: Copyright © 2025 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! `os-release` file support
//!
//! See the [freedesktop documentation](https://www.freedesktop.org/software/systemd/man/latest/os-release.html)
//! for more information.
//!
//! This crate supports fields pertaining to the use of os-release files within the context
//! of moss-managed distribution, and currently does not process any fields specifically
//! intended for container image builds.

use std::{collections::HashMap, str::FromStr};

use snafu::Snafu;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Missing key: {key}"))]
    MissingKey { key: &'static str },
}

/// Private helper to decode types from a map
/// Ok it's not the most efficient way, we could use Cow on a Read...
/// It just gets the job done.
trait MapDecode: Sized {
    fn map_decode(o: &HashMap<&str, &str>) -> Result<Self, self::Error>;
}

/// General structure of the `os-release` file used by Linux distributionss
#[derive(Debug)]
pub struct OsRelease {
    /// Name of the operating system
    pub name: String,

    /// Unique ID for the OS
    pub id: String,

    /// metadata
    pub meta: Metadata,

    /// versioning
    pub version: Version,

    /// Useful project/OS links
    pub urls: Urls,

    /// When does support end? ISO-8601
    pub support_ends: Option<String>,

    /// branding details
    pub brand: Brand,

    /// Vendor details
    pub vendor: Vendor,
}

impl FromStr for OsRelease {
    type Err = self::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let map = s
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.starts_with('#'))
            .filter_map(|s| s.split_once('='))
            .map(|(k, v)| (k, v.trim_matches(|c| c == '\'' || c == '\"')))
            .collect::<HashMap<_, _>>();

        Self::map_decode(&map)
    }
}

impl MapDecode for OsRelease {
    fn map_decode(o: &HashMap<&str, &str>) -> Result<Self, Error> {
        Ok(Self {
            name: o.get("NAME").ok_or(Error::MissingKey { key: "NAME" })?.to_string(),
            id: o.get("ID").ok_or(Error::MissingKey { key: "ID" })?.to_string(),
            meta: Metadata::map_decode(o)?,
            version: Version::map_decode(o)?,
            urls: Urls::map_decode(o)?,
            support_ends: o.get("SUPPORT_ENDS").map(|s| s.to_string()),
            brand: Brand::map_decode(o)?,
            vendor: Vendor::map_decode(o)?,
        })
    }
}

/// Logical grouping of metadata fields to assist in queries
#[derive(Debug)]
pub struct Metadata {
    /// What [`OsRelease::id`] is this OS like?
    pub like: Option<String>,

    /// "Nice" rendered name for the project
    pub pretty_name: Option<String>,

    /// [CPE](https://en.wikipedia.org/wiki/Common_Platform_Enumeration) "product name"
    pub cpe_name: Option<String>,
}

impl MapDecode for Metadata {
    fn map_decode(o: &HashMap<&str, &str>) -> Result<Self, Error> {
        Ok(Self {
            like: o.get("ID_LIKE").map(|s| s.to_string()),
            pretty_name: o.get("PRETTY_NAME").map(|s| s.to_string()),
            cpe_name: o.get("CPE_NAME").map(|s| s.to_string()),
        })
    }
}

/// Logical grouping of the distribution version data
#[derive(Debug)]
pub struct Version {
    /// Human readable display of version
    pub name: Option<String>,

    /// Unique ID for the version
    pub id: Option<String>,

    /// Any codename associated
    pub codename: Option<String>,

    /// To allow more enhanced project progress tracking, record the specific build ID
    pub build_id: Option<String>,

    /// Nane/description of variant author
    pub variant: Option<String>,

    /// Likewise, but a unique ID
    pub variant_id: Option<String>,
}

impl MapDecode for Version {
    fn map_decode(o: &HashMap<&str, &str>) -> Result<Self, Error> {
        Ok(Self {
            name: o.get("VERSION").map(|s| s.to_string()),
            id: o.get("VERSION_ID").map(|s| s.to_string()),
            codename: o.get("VERSION_CODENAME").map(|s| s.to_string()),
            build_id: o.get("BUILD_ID").map(|s| s.to_string()),
            variant: o.get("VARIANT").map(|s| s.to_string()),
            variant_id: o.get("VARIANT_ID").map(|s| s.to_string()),
        })
    }
}

/// Various URLs specific to the project
#[derive(Debug)]
pub struct Urls {
    /// public homepage
    pub homepage: Option<String>,

    /// Documentation resources
    pub documentation: Option<String>,

    /// Official support links/landing
    pub support: Option<String>,

    /// Where bugs may be reported (i.e. bugzilla/github)
    pub bug_report: Option<String>,

    /// Link to an up to date privacy policy for the distribution
    pub privacy_policy: Option<String>,
}

impl MapDecode for Urls {
    fn map_decode(o: &HashMap<&str, &str>) -> Result<Self, Error> {
        Ok(Self {
            homepage: o.get("HOME_URL").map(|s| s.to_string()),
            documentation: o.get("DOCUMENTATION_URL").map(|s| s.to_string()),
            support: o.get("SUPPORT_URL").map(|s| s.to_string()),
            bug_report: o.get("BUG_REPORT_URL").map(|s| s.to_string()),
            privacy_policy: o.get("PRIVACY_POLICY_URL").map(|s| s.to_string()),
        })
    }
}

/// Basic branding details (limited)
#[derive(Debug)]
pub struct Brand {
    /// A freedesktop icon naming spec compatible string for the distro logo
    pub logo: Option<String>,

    /// An ANSI sequence used to render the distro version/metadata on pty/tty
    pub ansi_color: Option<String>,
}

impl MapDecode for Brand {
    fn map_decode(o: &HashMap<&str, &str>) -> Result<Self, Error> {
        Ok(Self {
            logo: o.get("LOGO").map(|s| s.to_string()),
            ansi_color: o.get("ANSI_COLOR").map(|s| s.to_string()),
        })
    }
}

/// Vendor specific information
#[derive(Debug)]
pub struct Vendor {
    /// The shipping vendor's name
    pub name: Option<String>,

    /// The shipping vendor's website
    pub url: Option<String>,
}

impl MapDecode for Vendor {
    fn map_decode(o: &HashMap<&str, &str>) -> Result<Self, Error> {
        Ok(Self {
            name: o.get("name").map(|s| s.to_string()),
            url: o.get("url").map(|s| s.to_string()),
        })
    }
}
