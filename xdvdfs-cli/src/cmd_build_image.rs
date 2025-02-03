use std::{io::Write, path::PathBuf, str::FromStr};

use anyhow::anyhow;
use clap::{Args, Subcommand};
use maybe_async::maybe_async;
use xdvdfs::write::{
    self,
    fs::{RemapOverlayConfig, RemapOverlayFilesystem, StdFilesystem},
    img::ProgressInfo,
};

use crate::img::with_extension;

#[derive(Args)]
#[command(
    about = "Pack an image from a given specification",
    long_about = r#"Packs an image from a given specification
Unlike the `pack` command, `build-image` requires one or more map rules to be provided,
either in the form of an `xdvdfs.toml` file provided with the `-f` flag, or
as a series of `-m` arguments.

A map rule passed in a `-m` flag is of the form "host/**/path:image/path/{1}".
The host path matches a subpath to the `SOURCE_PATH`, supporting glob patterns.
This path is then rewritten to the image path. Within the rewritten path, glob
matches from the host can be rewritten using `{ }` directives containing the index
of the match, with the first match at index 1. Index `{0}` matches the entire host path.
Negative matches are supported by prepending '!' to the host path. The rewrite path is
not required and is ignored if the host path is a negative match.

In an `xdvdfs.toml` file, map rules can be specified in the `[map_rules]` table:
```
[map_rules]
"host/**/path" = "/image/path/{1}"
"specific/extensions/*.{abc,xyz}" = "/{1}/file.{2}"
"!negated/match" = ""
```

Use `xdvdfs image-spec from [OPTIONS]` to generate an `xdvdfs.toml` file from equivalent command options.
"#
)]
pub struct BuildImageArgs {
    #[arg(short = 'f', long = "file", help = "Path to image spec file")]
    image_spec: Option<String>,

    #[arg(
        short = 'm',
        long = "map",
        help = "Single map rule, of the form \"host/path:image/path\" or \"!excluded/host/path",
        conflicts_with = "image_spec"
    )]
    map_rules: Option<Vec<String>>,

    #[arg(
        short = 'O',
        long = "output",
        help = "Relative path to the resulting image output file",
        conflicts_with = "image_spec"
    )]
    meta_output: Option<String>,

    #[arg(
        short = 'D',
        long = "dry-run",
        help = "Output list of mapped files without packing image"
    )]
    dry_run: bool,

    #[arg(help = "Path to working directory. Defaults to current directory")]
    source_path: Option<String>,

    #[arg(help = "Path to output image. Inferred from source path or image spec if not provided")]
    image_path: Option<String>,
}

#[derive(Args)]
#[command(about = "Generate an `xdvdfs.toml` file from provided command options")]
pub struct ImageSpecFromArgs {
    #[arg(
        short = 'm',
        long = "map",
        help = "Single map rule, of the form \"host/path:image/path\" or \"!excluded/host/path\""
    )]
    map_rules: Option<Vec<String>>,

    #[arg(
        short = 'O',
        long = "output",
        help = "Relative path to the resulting image output file"
    )]
    meta_output: Option<String>,

    #[arg(help = "Path to output file. Output to stdout if not provided")]
    output_file: Option<String>,
}

#[derive(Subcommand)]
pub enum ImageSpecSubcommand {
    From(ImageSpecFromArgs),
}

#[derive(Args)]
#[command(about = "Manage image spec `xdvdfs.toml` files")]
pub struct ImageSpecArgs {
    #[command(subcommand)]
    command: ImageSpecSubcommand,
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
struct ImageInfo {
    output: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
struct ImageSpec {
    metadata: Option<ImageInfo>,
    map_rules: Option<toml::value::Table>,
}

fn deserialize_overlay_config(
    spec: &ImageSpec,
    map_rules: &mut Vec<(String, String)>,
) -> Result<(), anyhow::Error> {
    let Some(spec_rules) = &spec.map_rules else {
        return Ok(());
    };
    for (key, value) in spec_rules.iter() {
        let value = value
            .as_str()
            .ok_or(anyhow!("Invalid value type for key {key}, expected String"))?
            .to_owned();
        map_rules.push((key.to_owned(), value));
    }

    Ok(())
}

#[maybe_async]
pub async fn cmd_build_image(args: &BuildImageArgs) -> Result<(), anyhow::Error> {
    let source_path = if let Some(path) = &args.source_path {
        PathBuf::from_str(path)?
    } else {
        std::env::current_dir()?
    };

    let (spec_path, source_path) = if source_path.is_dir() {
        (source_path.join("xdvdfs.toml"), source_path)
    } else {
        let source = source_path
            .parent()
            .ok_or(anyhow!("Invalid source path"))?
            .to_path_buf();
        (source_path, source)
    };

    let spec_path = if let Some(path) = &args.image_spec {
        PathBuf::from_str(path)?
    } else {
        spec_path
    };

    let mut map_rules: Vec<(String, String)> = Vec::new();
    let mut image_spec: Option<ImageSpec> = None;

    if let Some(rules) = &args.map_rules {
        for rule in rules.iter() {
            let mut split = rule.split(":");
            let Some(host) = split.next() else {
                return Err(anyhow!("Map rule cannot be empty"));
            };
            let image = split.next();
            if image.is_none() && !host.starts_with('!') {
                return Err(anyhow!("Map rule \"{host}\" must have an image path unless it is an exclusion rule (starting with '!')"));
            }

            map_rules.push((
                host.to_owned(),
                image.map(|s| s.to_owned()).unwrap_or_default(),
            ));
        }
    }

    if spec_path.exists() && args.map_rules.is_none() {
        let spec = std::fs::read_to_string(&spec_path)?;
        let spec: ImageSpec = toml::from_str(&spec)?;
        deserialize_overlay_config(&spec, &mut map_rules)?;
        image_spec = Some(spec);
    }

    if map_rules.is_empty() {
        return Err(anyhow!("Must specify at least one map rule"));
    }

    let overlay_cfg = RemapOverlayConfig { map_rules };

    let image_path = if let Some(path) = &args.image_path {
        PathBuf::from_str(path)?
    } else {
        let output_path = image_spec
            .as_ref()
            .and_then(|spec| spec.metadata.as_ref())
            .and_then(|meta| meta.output.as_ref());

        if let Some(output) = output_path {
            source_path.join(output)
        } else {
            with_extension(&source_path, "xiso.iso", true)
        }
    };

    let stdfs = StdFilesystem::create(&source_path);
    let mut remapfs = RemapOverlayFilesystem::new(stdfs, overlay_cfg).await?;

    if args.dry_run {
        let mapped_entries = remapfs.dump();
        for (host, guest) in mapped_entries {
            println!("{} -> {}", host.as_string(), guest.as_string());
        }

        return Ok(());
    }

    let image = std::fs::File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(image_path)?;
    let mut image = std::io::BufWriter::with_capacity(1024 * 1024, image);

    let mut file_count: usize = 0;
    let mut progress_count: usize = 0;
    let progress_callback = |pi| match pi {
        ProgressInfo::FileCount(count) => file_count = count,
        ProgressInfo::DirCount(count) => file_count += count,
        ProgressInfo::DirAdded(path, sector) => {
            progress_count += 1;
            println!("[{progress_count}/{file_count}] Added dir: {path} at sector {sector}");
        }
        ProgressInfo::FileAdded(path, sector) => {
            progress_count += 1;
            println!("[{progress_count}/{file_count}] Added file: {path} at sector {sector}");
        }
        _ => {}
    };

    write::img::create_xdvdfs_image(&mut remapfs, &mut image, progress_callback).await?;

    Ok(())
}

#[maybe_async]
pub async fn cmd_image_spec(args: &ImageSpecArgs) -> Result<(), anyhow::Error> {
    let ImageSpecSubcommand::From(from_args) = &args.command;

    let map_rules = if let Some(rules) = &from_args.map_rules {
        let mut map_rules = toml::Table::default();
        for rule in rules.iter() {
            let mut split = rule.split(":");
            let Some(host) = split.next() else {
                return Err(anyhow!("Map rule cannot be empty"));
            };
            let image = split.next();
            if image.is_none() && !host.starts_with('!') {
                return Err(anyhow!("Map rule \"{host}\" must have an image path unless it is an exclusion rule (starting with '!')"));
            }

            map_rules.insert(
                host.to_owned(),
                toml::Value::String(image.map(|s| s.to_owned()).unwrap_or_default()),
            );
        }

        Some(map_rules)
    } else {
        None
    };

    let mut metadata = None;
    if from_args.meta_output.is_some() {
        metadata = Some(ImageInfo {
            output: from_args.meta_output.clone(),
        });
    }

    let spec = ImageSpec {
        metadata,
        map_rules,
    };
    let spec = toml::to_string_pretty(&spec)?;

    if let Some(output_file) = &from_args.output_file {
        std::fs::write(output_file, spec)?;
    } else {
        std::io::stdout().write_all(spec.as_bytes())?;
    }

    Ok(())
}
