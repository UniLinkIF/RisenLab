use risenlab::{batch, content, dds, gamepath, pak, ximg};

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "risenlab", about = "Risen 1 asset pipeline — core I/O layer")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all files inside a .pak / .pXX archive
    List { archive: PathBuf },
    /// Extract every file from a .pak / .pXX archive into a directory
    Unpack { archive: PathBuf, out_dir: PathBuf },
    /// Build a new .pak / .pXX archive from a directory (for patch volumes)
    Pack { in_dir: PathBuf, archive: PathBuf },
    /// Extract the embedded DDS payload from a Risen 1/2 ._ximg texture
    XimgToDds { input: PathBuf, output: PathBuf },
    /// Print ._ximg header info (width/height/offsets)
    XimgInfo { input: PathBuf },
    /// Splice a new DDS (same pixel format, e.g. after AI upscale) into a copy of an
    /// existing ._ximg, patching Width/Height in place.
    XimgPatch {
        input: PathBuf,
        new_dds: PathBuf,
        output: PathBuf,
        #[arg(long)]
        width: i32,
        #[arg(long)]
        height: i32,
        /// Only needed if the AI step changed the mip count
        #[arg(long)]
        skip_mips: Option<i32>,
        /// Only needed if the AI step changed the pixel format (e.g. DXT3 -> DXT5)
        #[arg(long)]
        pixel_format: Option<String>,
    },
    /// Point at risen.exe (or a .lnk shortcut to it) and find every archive in the install —
    /// this is the whole "pick the exe, we take it from there" flow.
    Discover { exe_or_shortcut: PathBuf },
    /// Decode a ._ximg texture's embedded DDS to a plain PNG — viewable and editable in any
    /// ordinary image tool, no DDS support required.
    XimgToPng { input: PathBuf, output: PathBuf },
    /// Splice a replacement PNG (any dimensions) into a copy of an existing ._ximg. Width,
    /// height and pixel format are all auto-detected — from the new PNG's dimensions and the
    /// original texture's own compression format — so no manual flags are needed.
    PngToXimg {
        input: PathBuf,
        new_png: PathBuf,
        output: PathBuf,
    },
    /// Point at the game (exe or .lnk) and extract every texture in every archive to a plain
    /// PNG, mirrored under `out_dir` with a manifest.tsv — the "whole game as a folder of
    /// photos to edit" step.
    ExtractTextures {
        exe_or_shortcut: PathBuf,
        out_dir: PathBuf,
    },
    /// Take a manifest from `extract-textures` plus a directory of (edited/regenerated) PNGs,
    /// and build fresh, minimal .pXX patch volumes containing only what actually changed.
    ApplyTextures {
        manifest: PathBuf,
        edited_dir: PathBuf,
        patch_out_dir: PathBuf,
    },
    /// Build a single self-contained HTML page showing original-vs-edited side by side for
    /// every texture that changed since extraction — open it in a browser to review before
    /// running apply-textures.
    ReviewTextures {
        manifest: PathBuf,
        edited_dir: PathBuf,
        out_html: PathBuf,
    },
    /// Export a `.xmsh` mesh to a standard `.obj`, editable in Blender or any 3D tool.
    /// Requires `mimicry-helper.exe` (see `../mimicry-helper`).
    MeshToObj { input: PathBuf, output: PathBuf },
    /// Import a standard `.obj` back into a `.xmsh` mesh. Requires `mimicry-helper.exe`.
    ObjToMesh { input: PathBuf, output: PathBuf },
    /// Dump every shader element and property of a `.xmat` material to a plain-text report.
    /// Requires `mimicry-helper.exe`.
    MaterialDump { input: PathBuf, output: PathBuf },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::List { archive } => {
            let mut a = pak::PakArchive::open(&archive)?;
            println!(
                "product={:#010x} valid_g3v0={} version={} revision={} encryption={} compression={} data_offset={} root_offset={} volume_size={}",
                a.header.product,
                a.is_valid_g3v0(),
                a.header.version,
                a.header.revision,
                a.header.encryption,
                a.header.compression,
                a.header.data_offset,
                a.header.root_offset,
                a.header.volume_size
            );
            let files = a.files();
            println!("{} files:", files.len());
            for f in &files {
                let tag = if f.is_deleted() { " [DELETED]" } else { "" };
                println!(
                    "  {}  data_size={} file_size={} compression={:?}{}",
                    f.path, f.data_size, f.file_size, f.compression, tag
                );
            }
            let _ = &mut a; // silence unused-mut style lints if any
        }
        Commands::Unpack { archive, out_dir } => {
            let mut a = pak::PakArchive::open(&archive)?;
            let count = a.extract_all(&out_dir)?;
            println!("Extracted {count} files to {}", out_dir.display());
        }
        Commands::Pack { in_dir, archive } => {
            pak::write_archive_from_dir(&in_dir, &archive)?;
            println!("Wrote {}", archive.display());
        }
        Commands::XimgToDds { input, output } => {
            let data = std::fs::read(&input)?;
            let dds = ximg::extract_dds(&data)?;
            std::fs::write(&output, dds)?;
            println!("Wrote {}", output.display());
        }
        Commands::XimgInfo { input } => {
            let data = std::fs::read(&input)?;
            let info = ximg::parse(&data)?;
            let pixel_format = ximg::read_pixel_format(&data).unwrap_or_else(|_| "?".to_string());
            println!("{info:#?}\npixel_format: {pixel_format}");
        }
        Commands::XimgPatch {
            input,
            new_dds,
            output,
            width,
            height,
            skip_mips,
            pixel_format,
        } => {
            let original = std::fs::read(&input)?;
            let dds = std::fs::read(&new_dds)?;
            let opts = ximg::ReplaceOptions {
                width,
                height,
                skip_mips,
                pixel_format: pixel_format.as_deref(),
            };
            let patched = ximg::replace_dds(&original, opts, &dds)?;
            std::fs::write(&output, &patched)?;
            println!(
                "Wrote {} ({} bytes, {}x{})",
                output.display(),
                patched.len(),
                width,
                height
            );
        }
        Commands::XimgToPng { input, output } => {
            let data = std::fs::read(&input)?;
            let dds_bytes = ximg::extract_dds(&data)?;
            let decoded = dds::decode(dds_bytes)?;
            let img = image::RgbaImage::from_raw(decoded.width, decoded.height, decoded.rgba)
                .ok_or_else(|| anyhow::anyhow!("decoded RGBA buffer does not match its own dimensions"))?;
            img.save(&output)?;
            println!(
                "Wrote {} ({}x{})",
                output.display(),
                decoded.width,
                decoded.height
            );
        }
        Commands::PngToXimg {
            input,
            new_png,
            output,
        } => {
            let original = std::fs::read(&input)?;
            let original_dds = ximg::extract_dds(&original)?;
            let original_parsed = ddsfile::Dds::read(original_dds)?;
            let format = dds::resolve_format(&original_parsed)
                .ok_or_else(|| anyhow::anyhow!("original texture's DDS pixel format is not a recognized D3D format"))?;

            let img = image::ImageReader::open(&new_png)?.decode()?.to_rgba8();
            let (width, height) = img.dimensions();
            let new_dds = dds::encode(width, height, img.as_raw(), format)?;

            let opts = ximg::ReplaceOptions {
                width: width as i32,
                height: height as i32,
                skip_mips: None,
                pixel_format: None,
            };
            let patched = ximg::replace_dds(&original, opts, &new_dds)?;
            std::fs::write(&output, &patched)?;
            println!(
                "Wrote {} ({} bytes, {}x{})",
                output.display(),
                patched.len(),
                width,
                height
            );
        }
        Commands::ExtractTextures {
            exe_or_shortcut,
            out_dir,
        } => {
            let count = batch::extract_all(&exe_or_shortcut, &out_dir)?;
            println!("Extracted {count} textures to {}", out_dir.display());
        }
        Commands::ApplyTextures {
            manifest,
            edited_dir,
            patch_out_dir,
        } => {
            let written = batch::apply(&manifest, &edited_dir, &patch_out_dir)?;
            if written.is_empty() {
                println!("No changed textures found — nothing to patch.");
            } else {
                println!("Wrote {} patch volume(s):", written.len());
                for p in &written {
                    println!("  {}", p.display());
                }
            }
        }
        Commands::ReviewTextures {
            manifest,
            edited_dir,
            out_html,
        } => {
            let count = batch::build_review_html(&manifest, &edited_dir, &out_html)?;
            println!("Wrote {} ({count} changed texture(s))", out_html.display());
        }
        Commands::MeshToObj { input, output } => {
            content::mesh_to_obj(&input, &output)?;
            println!("Wrote {}", output.display());
        }
        Commands::ObjToMesh { input, output } => {
            content::obj_to_mesh(&input, &output)?;
            println!("Wrote {}", output.display());
        }
        Commands::MaterialDump { input, output } => {
            content::material_dump(&input, &output)?;
            println!("Wrote {}", output.display());
        }
        Commands::Discover { exe_or_shortcut } => {
            let exe = gamepath::resolve_shortcut(&exe_or_shortcut)?;
            println!("Resolved target: {}", exe.display());
            let root = gamepath::discover_game_root(&exe).ok_or_else(|| {
                anyhow::anyhow!("could not find a data/ folder with archives above {}", exe.display())
            })?;
            println!("Game root: {}", root.display());
            let archives = gamepath::discover_archives(&root)?;
            println!("{} archives found:", archives.len());
            for a in &archives {
                println!("  [{}] {}", a.group, a.path.display());
            }
        }
    }
    Ok(())
}
