//! `cargo set-version`
#![warn(
    missing_docs,
    missing_debug_implementations,
    missing_copy_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    unused_qualifications
)]

#[macro_use]
extern crate error_chain;

use crate::errors::*;
use cargo_edit::{find, LocalManifest};
use failure::Fail;
use semver::Version;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use structopt::StructOpt;
use termcolor::{BufferWriter, Color, ColorChoice, ColorSpec, WriteColor};

mod errors {
    error_chain! {
        links {
            CargoEditLib(::cargo_edit::Error, ::cargo_edit::ErrorKind);
        }
        foreign_links {
            CargoMetadata(::failure::Compat<::cargo_metadata::Error>);
        }
    }
}

#[derive(Debug, StructOpt)]
#[structopt(bin_name = "cargo")]
enum Command {
    /// Sets the version specified in the local manifest file (i.e. Cargo.toml).
    #[structopt(name = "set-version")]
    SetVersion(Args),
}

#[derive(Debug, StructOpt)]
struct Args {
    /// Path to the manifest to set version
    #[structopt(long = "manifest-path", value_name = "path")]
    manifest_path: Option<PathBuf>,

    /// Set version of all packages in the workspace.
    #[structopt(long = "workspace")]
    all: bool,

    /// Set a specific semantic version.
    #[structopt(short, long = "version")]
    version: Version,

    /// Print changes to be made without making them.
    #[structopt(long = "dry-run")]
    dry_run: bool,
}

/// A collection of manifests.
struct Manifests(Vec<(LocalManifest, cargo_metadata::Package)>);

fn dry_run_message() -> Result<()> {
    let bufwtr = BufferWriter::stdout(ColorChoice::Auto);
    let mut buffer = bufwtr.buffer();
    buffer
        .set_color(ColorSpec::new().set_fg(Some(Color::Cyan)).set_bold(true))
        .chain_err(|| "Failed to set output colour")?;
    write!(&mut buffer, "Starting dry run. ").chain_err(|| "Failed to write dry run message")?;
    buffer
        .set_color(&ColorSpec::new())
        .chain_err(|| "Failed to clear output colour")?;
    writeln!(&mut buffer, "Changes will not be saved.")
        .chain_err(|| "Failed to write dry run message")?;
    bufwtr
        .print(&buffer)
        .chain_err(|| "Failed to print dry run message")
}

impl Manifests {
    /// Get all manifests in the workspace.
    fn get_all(manifest_path: &Option<PathBuf>) -> Result<Self> {
        let mut cmd = cargo_metadata::MetadataCommand::new();
        cmd.no_deps();
        if let Some(path) = manifest_path {
            cmd.manifest_path(path);
        }
        let result = cmd.exec().map_err(|e| {
            Error::from(e.compat()).chain_err(|| "Failed to get workspace metadata")
        })?;
        result
            .packages
            .into_iter()
            .map(|package| {
                Ok((
                    LocalManifest::try_new(Path::new(&package.manifest_path))?,
                    package,
                ))
            })
            .collect::<Result<Vec<_>>>()
            .map(Manifests)
    }

    /// Get the manifest specified by the manifest path. Try to make an educated guess if no path is
    /// provided.
    fn get_local_one(manifest_path: &Option<PathBuf>) -> Result<Self> {
        let resolved_manifest_path: String = find(&manifest_path)?.to_string_lossy().into();

        let manifest = LocalManifest::find(&manifest_path)?;

        let mut cmd = cargo_metadata::MetadataCommand::new();
        cmd.no_deps();
        if let Some(path) = manifest_path {
            cmd.manifest_path(path);
        }
        let result = cmd
            .exec()
            .map_err(|e| Error::from(e.compat()).chain_err(|| "Invalid manifest"))?;
        let packages = result.packages;
        let package = packages
            .iter()
            .find(|p| p.manifest_path.to_string_lossy() == resolved_manifest_path)
            // If we have successfully got metadata, but our manifest path does not correspond to a
            // package, we must have been called against a virtual manifest.
            .chain_err(|| {
                "Found virtual manifest, but this command requires running against an \
                 actual package in this workspace. Try adding `--workspace`."
            })?;

        Ok(Manifests(vec![(manifest, package.to_owned())]))
    }

    /// Update the manifests on disk with the given version.
    fn set_version(self, dry_run: bool, version: Version) -> Result<()> {
        if dry_run {
            dry_run_message()?;
        }
        for (mut manifest, package) in self.0 {
            let bufwtr = BufferWriter::stdout(ColorChoice::Always);
            let mut buffer = bufwtr.buffer();
            writeln!(
                &mut buffer,
                "{}: {} -> {}",
                package.name, package.version, version
            )
            .and_then(|_| bufwtr.print(&buffer))
            .chain_err(|| "Failed to print changes")?;
            manifest.set_version(dry_run, &version)?;
        }
        Ok(())
    }
}

/// Main processing function. Allows us to return a `Result` so that `main` can print pretty error
/// messages.
fn process(args: Args) -> Result<()> {
    let Args {
        manifest_path,
        all,
        version,
        dry_run,
    } = args;

    let manifests = if all {
        Manifests::get_all(&manifest_path)
    } else {
        Manifests::get_local_one(&manifest_path)
    }?;

    manifests.set_version(dry_run, version)
}

fn main() {
    let args: Command = Command::from_args();
    let Command::SetVersion(args) = args;

    if let Err(err) = process(args) {
        eprintln!("Command failed due to unhandled error: {}\n", err);

        for e in err.iter().skip(1) {
            eprintln!("Caused by: {}", e);
        }

        if let Some(backtrace) = err.backtrace() {
            eprintln!("Backtrace: {:?}", backtrace);
        }

        process::exit(1);
    }
}
