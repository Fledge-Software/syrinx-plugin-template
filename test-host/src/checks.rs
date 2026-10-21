// SPDX-License-Identifier: MPL-2.0
//! Checks made before instantiation: the package, the manifest and the component agree.

use plugin_format::{Manifest, PackageList};
use wasmtime::Engine;
use wasmtime::component::Component;

use crate::HostError;

/// The interfaces every plugin exports, without their version.
const REQUIRED_EXPORTS: &[&str] = &[
    "lifecycle",
    "commands",
    "forms",
    "views",
    "search-feed",
    "events",
];

/// `syrinx:plugin` interfaces any plugin may import without a capability.
const UNCONDITIONAL_IMPORTS: &[&str] = &["types", "host"];

/// WASI 0.2 packages Rust's standard library links. They are linked with an
/// empty context; files, sockets and HTTP are never linked for a plugin.
const ALLOWED_WASI_PACKAGES: &[&str] = &["wasi:cli/", "wasi:io/", "wasi:clocks/", "wasi:random/"];

/// The signed list, the manifest and the release must name the same plugin.
pub fn package_matches_manifest(
    package: &PackageList,
    manifest: &Manifest,
) -> Result<(), HostError> {
    let pairs = [
        ("id", package.id.as_str(), manifest.plugin.id.as_str()),
        (
            "version",
            package.version.as_str(),
            manifest.plugin.version.as_str(),
        ),
        (
            "world",
            package.world.as_str(),
            manifest.interface.world.as_str(),
        ),
    ];
    for (field, in_package, in_manifest) in pairs {
        if in_package != in_manifest {
            return Err(HostError::Refused(format!(
                "the package says {field} {in_package} but the manifest says {in_manifest}"
            )));
        }
    }
    if package.file(&manifest.entry.module).is_none() {
        return Err(HostError::Refused(
            "the module is not in the package list".into(),
        ));
    }
    Ok(())
}

/// Native code is only ever compiled on the owner's machine, never shipped.
pub fn refuse_precompiled(module: &[u8]) -> Result<(), HostError> {
    if Engine::detect_precompiled(module).is_some() {
        return Err(HostError::Component(
            "the module is precompiled native code".into(),
        ));
    }
    Ok(())
}

/// The component exports the manifest's world and imports nothing it did not ask for.
pub fn component_matches_manifest(
    engine: &Engine,
    component: &Component,
    manifest: &Manifest,
) -> Result<(), HostError> {
    let version = manifest.world_version();
    let component_type = component.component_type();
    let exports: Vec<&str> = component_type
        .exports(engine)
        .map(|(name, _)| name)
        .collect();
    for interface in REQUIRED_EXPORTS {
        let expected = format!("syrinx:plugin/{interface}@{version}");
        if !exports.contains(&expected.as_str()) {
            return Err(HostError::Component(format!(
                "the component does not export {expected}"
            )));
        }
    }
    for (name, _) in component_type.imports(engine) {
        check_import(name, version, manifest)?;
    }
    Ok(())
}

fn check_import(name: &str, version: &str, manifest: &Manifest) -> Result<(), HostError> {
    if let Some(rest) = name.strip_prefix("syrinx:plugin/") {
        return check_syrinx_import(rest, version, manifest);
    }
    let allowed_wasi = name.contains("@0.2.")
        && ALLOWED_WASI_PACKAGES
            .iter()
            .any(|prefix| name.starts_with(prefix));
    if allowed_wasi {
        return Ok(());
    }
    Err(HostError::Component(format!(
        "the component imports {name}, which no plugin is given"
    )))
}

/// A capability interface (none exist in 0.1.0 yet) needs a matching request.
fn check_syrinx_import(rest: &str, version: &str, manifest: &Manifest) -> Result<(), HostError> {
    let Some((interface, imported_version)) = rest.split_once('@') else {
        return Err(HostError::Component(format!(
            "import syrinx:plugin/{rest} has no version"
        )));
    };
    if imported_version != version {
        return Err(HostError::Component(format!(
            "import {interface} is at {imported_version}, the manifest says {version}"
        )));
    }
    if UNCONDITIONAL_IMPORTS.contains(&interface) || manifest.requests(interface) {
        return Ok(());
    }
    Err(HostError::Component(format!(
        "the component imports {interface}, which its manifest does not request"
    )))
}
