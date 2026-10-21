// SPDX-License-Identifier: MPL-2.0
//! The two file formats a plugin package carries, shared by the build tool and the test host.

pub mod manifest;
pub mod package;

pub use manifest::{Manifest, ManifestError};
pub use package::{FileEntry, PackageError, PackageList};
