mod pak;
mod ximg;

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
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::List { archive } => {
            let mut a = pak::PakArchive::open(&archive)?;
            println!(
                "product={:#010x} valid_g3v0={} data_offset={} root_offset={} volume_size={}",
                a.header.product,
                a.is_valid_g3v0(),
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
            println!("{info:#?}");
        }
        Commands::XimgPatch {
            input,
            new_dds,
            output,
            width,
            height,
        } => {
            let original = std::fs::read(&input)?;
            let dds = std::fs::read(&new_dds)?;
            let patched = ximg::replace_dds(&original, width, height, &dds)?;
            std::fs::write(&output, &patched)?;
            println!(
                "Wrote {} ({} bytes, {}x{})",
                output.display(),
                patched.len(),
                width,
                height
            );
        }
    }
    Ok(())
}
