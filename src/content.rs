//! Materials/meshes/animations support via `mimicry-helper` — a separately-built, GPL-3.0
//! executable (see `../mimicry-helper`, sibling to this repo) called out-of-process, so this
//! crate itself never links GPL code (see docs/formats/content-layer.md for why).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Locates `mimicry-helper.exe`. Checked in order: `RISENLAB_MIMICRY_HELPER` env var, next to
/// this binary's own directory, next to this repo (`../mimicry-helper`, dev layout), then the
/// known fixed location on this machine — needed because a copy of `risenlab_gui.exe` (e.g. on
/// the Desktop) has neither a meaningful "current directory" nor "next to the repo" relationship
/// to the helper.
fn helper_exe_path() -> PathBuf {
    if let Ok(p) = std::env::var("RISENLAB_MIMICRY_HELPER") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("mimicry-helper.exe");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    let relative = PathBuf::from("../mimicry-helper/mimicry-helper.exe");
    if relative.exists() {
        return relative;
    }
    PathBuf::from(r"C:\Users\rusak\OneDrive\Desktop\Claude\mimicry-helper\mimicry-helper.exe")
}

fn run_helper(args: &[&str]) -> Result<()> {
    let exe = helper_exe_path();
    if !exe.exists() {
        bail!(
            "mimicry-helper.exe not found at {} (build it in ../mimicry-helper, or set RISENLAB_MIMICRY_HELPER)",
            exe.display()
        );
    }
    let status = Command::new(&exe)
        .args(args)
        .status()
        .with_context(|| format!("running {}", exe.display()))?;
    if !status.success() {
        bail!("mimicry-helper exited with {status}");
    }
    Ok(())
}

/// Exports a `.xmsh` mesh to a standard `.obj` (editable in Blender or any 3D tool).
pub fn mesh_to_obj(input: &Path, output: &Path) -> Result<()> {
    run_helper(&["mesh-to-obj", &input.to_string_lossy(), &output.to_string_lossy()])
}

/// Imports a standard `.obj` back into a `.xmsh` mesh.
pub fn obj_to_mesh(input: &Path, output: &Path) -> Result<()> {
    run_helper(&["obj-to-mesh", &input.to_string_lossy(), &output.to_string_lossy()])
}

/// Dumps every shader element and property of a `.xmat` material to a plain-text report.
pub fn material_dump(input: &Path, output: &Path) -> Result<()> {
    run_helper(&["material-dump", &input.to_string_lossy(), &output.to_string_lossy()])
}
