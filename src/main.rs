//! Build and publish crates with pyo3, rust-cpython and cffi bindings as well as rust binaries
//! as python packages. This file contains the CLI and keyring integration.
//!
//! Run with --help for usage information

use anyhow::{bail, Context, Result};
use cargo_zigbuild::Zig;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Generator;
use maturin::{
    develop, init_project, new_project, write_dist_info, BridgeModel, BuildOptions, CargoOptions,
    GenerateProjectOptions, PathWriter, PlatformTag, PythonInterpreter, Target,
};
#[cfg(feature = "upload")]
use maturin::{upload_ui, PublishOpt};
use std::env;
use std::io;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Parser)]
#[command(
    version,
    name = env!("CARGO_PKG_NAME"),
    display_order = 1,
)]
#[cfg_attr(feature = "cargo-clippy", allow(clippy::large_enum_variant))]
/// Build and publish crates with pyo3, rust-cpython and cffi bindings as well
/// as rust binaries as python packages
enum Opt {
    #[command(name = "build", alias = "b")]
    /// Build the crate into python packages
    Build {
        /// Build artifacts in release mode, with optimizations
        #[arg(short = 'r', long)]
        release: bool,
        /// Strip the library for minimum file size
        #[arg(long)]
        strip: bool,
        /// Build a source distribution
        #[arg(long)]
        sdist: bool,
        #[command(flatten)]
        build: BuildOptions,
    },
    #[cfg(feature = "upload")]
    #[command(name = "publish")]
    /// Build and publish the crate as python packages to pypi
    Publish {
        /// Do not pass --release to cargo
        #[arg(long)]
        debug: bool,
        /// Do not strip the library for minimum file size
        #[arg(long = "no-strip")]
        no_strip: bool,
        /// Don't build a source distribution
        #[arg(long = "no-sdist")]
        no_sdist: bool,
        #[command(flatten)]
        publish: PublishOpt,
        #[command(flatten)]
        build: BuildOptions,
    },
    #[command(name = "list-python")]
    /// Search and list the available python installations
    ListPython {
        #[arg(long)]
        target: Option<String>,
    },
    #[command(name = "develop", alias = "dev")]
    /// Install the crate as module in the current virtualenv
    ///
    /// Note that this command doesn't create entrypoints
    Develop {
        /// Which kind of bindings to use. Possible values are pyo3, rust-cpython, cffi and bin
        #[arg(short = 'b', long = "bindings", alias = "binding-crate")]
        bindings: Option<String>,
        /// Pass --release to cargo
        #[arg(short = 'r', long)]
        release: bool,
        /// Strip the library for minimum file size
        #[arg(long)]
        strip: bool,
        /// Install extra requires aka. optional dependencies
        ///
        /// Use as `--extras=extra1,extra2`
        #[arg(
            short = 'E',
            long,
            value_delimiter = ',',
            action = clap::ArgAction::Append
        )]
        extras: Vec<String>,
        #[command(flatten)]
        cargo_options: CargoOptions,
    },
    /// Build only a source distribution (sdist) without compiling.
    ///
    /// Building a source distribution requires a pyproject.toml with a `[build-system]` table.
    ///
    /// This command is a workaround for [pypa/pip#6041](https://github.com/pypa/pip/issues/6041)
    #[command(name = "sdist")]
    SDist {
        #[arg(short = 'm', long = "manifest-path")]
        /// The path to the Cargo.toml
        manifest_path: Option<PathBuf>,
        /// The directory to store the built wheels in. Defaults to a new "wheels"
        /// directory in the project's target directory
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Create a new cargo project in an existing directory
    #[command(name = "init")]
    InitProject {
        /// Project path
        path: Option<String>,
        #[command(flatten)]
        options: GenerateProjectOptions,
    },
    /// Create a new cargo project
    #[command(name = "new")]
    NewProject {
        /// Project path
        path: String,
        #[command(flatten)]
        options: GenerateProjectOptions,
    },
    /// Upload python packages to pypi
    ///
    /// It is mostly similar to `twine upload`, but can only upload python wheels
    /// and source distributions.
    #[cfg(feature = "upload")]
    #[command(name = "upload")]
    Upload {
        #[command(flatten)]
        publish: PublishOpt,
        /// The python packages to upload
        #[arg(value_name = "FILE")]
        files: Vec<PathBuf>,
    },
    /// Backend for the PEP 517 integration. Not for human consumption
    ///
    /// The commands are meant to be called from the python PEP 517
    #[command(subcommand)]
    Pep517(Pep517Command),
    /// Generate shell completions
    #[command(name = "completions", hide = true)]
    Completions {
        #[arg(value_name = "SHELL")]
        shell: Shell,
    },
    /// Zig linker wrapper
    #[command(subcommand, hide = true)]
    Zig(Zig),
}

/// Backend for the PEP 517 integration. Not for human consumption
///
/// The commands are meant to be called from the python PEP 517
#[derive(Debug, Subcommand)]
#[command(name = "pep517", hide = true)]
enum Pep517Command {
    /// The implementation of prepare_metadata_for_build_wheel
    #[command(name = "write-dist-info")]
    WriteDistInfo {
        #[command(flatten)]
        build_options: BuildOptions,
        /// The metadata_directory argument to prepare_metadata_for_build_wheel
        #[arg(long = "metadata-directory")]
        metadata_directory: PathBuf,
        /// Strip the library for minimum file size
        #[arg(long)]
        strip: bool,
    },
    #[command(name = "build-wheel")]
    /// Implementation of build_wheel
    ///
    /// --release and --strip are currently unused by the PEP 517 implementation
    BuildWheel {
        #[command(flatten)]
        build_options: BuildOptions,
        /// Strip the library for minimum file size
        #[arg(long)]
        strip: bool,
        /// Build editable wheels
        #[arg(long)]
        editable: bool,
    },
    /// The implementation of build_sdist
    #[command(name = "write-sdist")]
    WriteSDist {
        /// The sdist_directory argument to build_sdist
        #[arg(long = "sdist-directory")]
        sdist_directory: PathBuf,
        #[arg(
            short = 'm',
            long = "manifest-path",
            default_value = "Cargo.toml",
            value_name = "PATH"
        )]
        /// The path to the Cargo.toml
        manifest_path: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[allow(clippy::enum_variant_names)]
enum Shell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
    Fig,
}

impl FromStr for Shell {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "bash" => Ok(Shell::Bash),
            "elvish" => Ok(Shell::Elvish),
            "fish" => Ok(Shell::Fish),
            "powershell" => Ok(Shell::PowerShell),
            "zsh" => Ok(Shell::Zsh),
            "fig" => Ok(Shell::Fig),
            _ => Err("[valid values: bash, elvish, fish, powershell, zsh, fig]".to_string()),
        }
    }
}

/// Dispatches into the native implementations of the PEP 517 functions
///
/// The last line of stdout is used as return value from the python part of the implementation
fn pep517(subcommand: Pep517Command) -> Result<()> {
    match subcommand {
        Pep517Command::WriteDistInfo {
            build_options,
            metadata_directory,
            strip,
        } => {
            assert_eq!(build_options.interpreter.len(), 1);
            let context = build_options.into_build_context(true, strip, false)?;

            // Since afaik all other PEP 517 backends also return linux tagged wheels, we do so too
            let tags = match context.bridge {
                BridgeModel::Bindings(..) | BridgeModel::Bin(Some(..)) => {
                    vec![context.interpreter[0].get_tag(
                        &context.target,
                        &[PlatformTag::Linux],
                        context.universal2,
                    )?]
                }
                BridgeModel::BindingsAbi3(major, minor) => {
                    let platform = context
                        .target
                        .get_platform_tag(&[PlatformTag::Linux], context.universal2)?;
                    vec![format!("cp{}{}-abi3-{}", major, minor, platform)]
                }
                BridgeModel::Bin(None) | BridgeModel::Cffi => {
                    context
                        .target
                        .get_universal_tags(&[PlatformTag::Linux], context.universal2)?
                        .1
                }
            };

            let mut writer = PathWriter::from_path(metadata_directory);
            write_dist_info(&mut writer, &context.metadata21, &tags)?;
            println!("{}", context.metadata21.get_dist_info_dir().display());
        }
        Pep517Command::BuildWheel {
            build_options,
            strip,
            editable,
        } => {
            let build_context = build_options.into_build_context(true, strip, editable)?;
            let wheels = build_context.build_wheels()?;
            assert_eq!(wheels.len(), 1);
            println!("{}", wheels[0].0.to_str().unwrap());
        }
        Pep517Command::WriteSDist {
            sdist_directory,
            manifest_path,
        } => {
            let build_options = BuildOptions {
                out: Some(sdist_directory),
                cargo: CargoOptions {
                    manifest_path: Some(manifest_path),
                    ..Default::default()
                },
                ..Default::default()
            };
            let build_context = build_options.into_build_context(false, false, false)?;
            let (path, _) = build_context
                .build_source_distribution()?
                .context("Failed to build source distribution, pyproject.toml not found")?;
            println!("{}", path.file_name().unwrap().to_str().unwrap());
        }
    };

    Ok(())
}

fn run() -> Result<()> {
    #[cfg(feature = "log")]
    tracing_subscriber::fmt::init();

    // Allow symlink `maturin` to `ar` to invoke `zig ar`
    // See https://github.com/messense/cargo-zigbuild/issues/52
    let mut args = env::args();
    let program_path = PathBuf::from(args.next().expect("no program path"));
    let program_name = program_path.file_stem().expect("no program name");
    if program_name.eq_ignore_ascii_case("ar") {
        let zig = Zig::Ar {
            args: args.collect(),
        };
        zig.execute()?;
        return Ok(());
    }

    let opt = Opt::parse();

    match opt {
        Opt::Build {
            build,
            release,
            strip,
            sdist,
        } => {
            let build_context = build.into_build_context(release, strip, false)?;
            if sdist {
                build_context
                    .build_source_distribution()?
                    .context("Failed to build source distribution, pyproject.toml not found")?;
            }
            let wheels = build_context.build_wheels()?;
            assert!(!wheels.is_empty());
        }
        #[cfg(feature = "upload")]
        Opt::Publish {
            build,
            publish,
            debug,
            no_strip,
            no_sdist,
        } => {
            let build_context = build.into_build_context(!debug, !no_strip, false)?;

            if !build_context.release {
                eprintln!("⚠️  Warning: You're publishing debug wheels");
            }

            let mut wheels = build_context.build_wheels()?;
            if !no_sdist {
                if let Some(sd) = build_context.build_source_distribution()? {
                    wheels.push(sd);
                }
            }

            let items = wheels.into_iter().map(|wheel| wheel.0).collect::<Vec<_>>();

            upload_ui(&items, &publish)?
        }
        Opt::ListPython { target } => {
            let found = if target.is_some() {
                let target = Target::from_target_triple(target)?;
                PythonInterpreter::find_by_target(&target, None)
            } else {
                let target = Target::from_target_triple(None)?;
                // We don't know the targeted bindings yet, so we use the most lenient
                PythonInterpreter::find_all(&target, &BridgeModel::Cffi, None)?
            };
            println!("🐍 {} python interpreter found:", found.len());
            for interpreter in found {
                println!(" - {}", interpreter);
            }
        }
        Opt::Develop {
            bindings,
            release,
            strip,
            extras,
            cargo_options,
        } => {
            let venv_dir = match (env::var_os("VIRTUAL_ENV"), env::var_os("CONDA_PREFIX")) {
                (Some(dir), None) => PathBuf::from(dir),
                (None, Some(dir)) => PathBuf::from(dir),
                (Some(_), Some(_)) => {
                    bail!("Both VIRTUAL_ENV and CONDA_PREFIX are set. Please unset one of them")
                }
                (None, None) => {
                    bail!(
                        "You need to be inside a virtualenv or conda environment to use develop \
                        (neither VIRTUAL_ENV nor CONDA_PREFIX are set). \
                        See https://virtualenv.pypa.io/en/latest/index.html on how to use virtualenv or \
                        use `maturin build` and `pip install <path/to/wheel>` instead."
                    )
                }
            };

            develop(bindings, cargo_options, &venv_dir, release, strip, extras)?;
        }
        Opt::SDist { manifest_path, out } => {
            let build_options = BuildOptions {
                out,
                cargo: CargoOptions {
                    manifest_path,
                    ..Default::default()
                },
                ..Default::default()
            };
            let build_context = build_options.into_build_context(false, false, false)?;
            build_context
                .build_source_distribution()?
                .context("Failed to build source distribution, pyproject.toml not found")?;
        }
        Opt::Pep517(subcommand) => pep517(subcommand)?,
        Opt::InitProject { path, options } => init_project(path, options)?,
        Opt::NewProject { path, options } => new_project(path, options)?,
        #[cfg(feature = "upload")]
        Opt::Upload { publish, files } => {
            if files.is_empty() {
                eprintln!("⚠️  Warning: No files given, exiting.");
                return Ok(());
            }

            upload_ui(&files, &publish)?
        }
        Opt::Completions { shell } => {
            let mut cmd = Opt::command();
            match shell {
                Shell::Fig => {
                    cmd.set_bin_name(env!("CARGO_BIN_NAME"));
                    let fig = clap_complete_fig::Fig;
                    fig.generate(&cmd, &mut io::stdout());
                }
                _ => {
                    let shell = match shell {
                        Shell::Bash => clap_complete::Shell::Bash,
                        Shell::Elvish => clap_complete::Shell::Elvish,
                        Shell::Fish => clap_complete::Shell::Fish,
                        Shell::PowerShell => clap_complete::Shell::PowerShell,
                        Shell::Zsh => clap_complete::Shell::Zsh,
                        Shell::Fig => unreachable!(),
                    };
                    clap_complete::generate(
                        shell,
                        &mut cmd,
                        env!("CARGO_BIN_NAME"),
                        &mut io::stdout(),
                    )
                }
            }
        }
        Opt::Zig(subcommand) => {
            subcommand
                .execute()
                .context("Failed to run zig linker wrapper")?;
        }
    }

    Ok(())
}

fn main() {
    #[cfg(feature = "human-panic")]
    {
        human_panic::setup_panic!();
    }

    if let Err(e) = run() {
        eprintln!("💥 maturin failed");
        for cause in e.chain().collect::<Vec<_>>().iter() {
            eprintln!("  Caused by: {}", cause);
        }
        std::process::exit(1);
    }
}
