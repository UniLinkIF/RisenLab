//! Materials/meshes/animations support via `mimicry-helper` — a separately-built, GPL-3.0
//! executable (see `../mimicry-helper`, sibling to this repo) called out-of-process, so this
//! crate itself never links GPL code (see docs/formats/content-layer.md for why).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Stops a spawned child process (`curl.exe`, `mimicry-helper.exe`) from popping up its own
/// console window for the second or so it runs. The packaged app (`risenlab-ui.exe`) is a
/// windowless GUI process, so by Windows' default console-inheritance rules, any child that
/// itself expects a console gets a BRAND NEW one allocated and briefly shown — a real,
/// owner-reported bug ("на кожному ревю вибиває на весь екран ніби консоль на секунду"),
/// triggered on every AI regenerate (each is 1-3 curl.exe calls: create/poll/download).
/// `CREATE_NO_WINDOW` (0x08000000) tells `CreateProcess` not to allocate one at all.
pub fn suppress_console_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// Locates `mimicry-helper.exe`. Checked in order: `RISENLAB_MIMICRY_HELPER` env var, next to
/// this binary's own directory (the packaged-install case — ship `mimicry-helper.exe` alongside
/// `risenlab-ui.exe`/`risenlab.exe`), then next to this repo (`../mimicry-helper`, dev layout).
/// If none of those exist, returns the "next to this binary" candidate anyway so `run_helper`'s
/// not-found error reports a path meaningful on WHOEVER's machine is running it — a previous
/// version fell back to a hardcoded path on the original dev machine, which was silently wrong
/// (and confusing to debug) for any other install.
fn helper_exe_path() -> PathBuf {
    if let Ok(p) = std::env::var("RISENLAB_MIMICRY_HELPER") {
        return PathBuf::from(p);
    }
    let next_to_exe = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join("mimicry-helper.exe")));
    if let Some(candidate) = &next_to_exe {
        if candidate.exists() {
            return candidate.clone();
        }
    }
    let relative = PathBuf::from("../mimicry-helper/mimicry-helper.exe");
    if relative.exists() {
        return relative;
    }
    next_to_exe.unwrap_or(relative)
}

fn run_helper(args: &[&str]) -> Result<()> {
    let exe = helper_exe_path();
    if !exe.exists() {
        bail!(
            "mimicry-helper.exe not found at {} (build it in ../mimicry-helper, or set RISENLAB_MIMICRY_HELPER)",
            exe.display()
        );
    }
    // NOT `.status()` — that inherits this process's own stdout, and mimicry-helper's driver
    // prints its own "Wrote <path>" line there. Every caller of `mesh_to_obj`/`actor_to_obj`
    // then prints its OWN real output (a JSON-encoded path) to the same stdout right after —
    // this app's dev-server/Tauri layer reads that combined, two-line stdout and does a plain
    // `JSON.parse` on it, which breaks the instant mimicry-helper's own line lands first (a
    // real regression: confirmed live, `/api/mesh-obj` started returning "Unexpected token 'W'"
    // after mimicry-helper.exe was rebuilt with the `actor-to-obj` addition). Capture the
    // child's stdout separately instead of inheriting it, so only this process's own,
    // intentional output ever reaches its real stdout.
    let mut cmd = Command::new(&exe);
    cmd.args(args);
    suppress_console_window(&mut cmd);
    let output = cmd.output().with_context(|| format!("running {}", exe.display()))?;
    if !output.status.success() {
        // stderr is captured now instead of inherited (see above), so it must be threaded into
        // the error message explicitly or a real mimicry-helper failure reason is lost.
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("mimicry-helper exited with {}: {}", output.status, stderr.trim());
    }
    Ok(())
}

/// Exports a `.xmsh` mesh to a standard `.obj` (editable in Blender or any 3D tool).
pub fn mesh_to_obj(input: &Path, output: &Path) -> Result<()> {
    run_helper(&["mesh-to-obj", &input.to_string_lossy(), &output.to_string_lossy()])
}

/// Exports a `.xmac` actor (skeleton + bind-pose mesh + materials) to a standard `.obj`.
/// Requires a `mimicry-helper.exe` built after the `actor-to-obj` driver command was added
/// (2026-07-15) — older builds only know `mesh-to-obj`/`obj-to-mesh`/`material-dump`.
pub fn actor_to_obj(input: &Path, output: &Path) -> Result<()> {
    run_helper(&["actor-to-obj", &input.to_string_lossy(), &output.to_string_lossy()])
}

/// Imports a standard `.obj` back into a `.xmsh` mesh.
pub fn obj_to_mesh(input: &Path, output: &Path) -> Result<()> {
    run_helper(&["obj-to-mesh", &input.to_string_lossy(), &output.to_string_lossy()])
}

/// Dumps every shader element and property of a `.xmat` material to a plain-text report.
pub fn material_dump(input: &Path, output: &Path) -> Result<()> {
    run_helper(&["material-dump", &input.to_string_lossy(), &output.to_string_lossy()])
}
