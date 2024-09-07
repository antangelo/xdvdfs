# xdvdfs

`xdvdfs` is a collection of tools for interacting with XDVDFS/XISO images.

## xdvdfs-cli

`xdvdfs-cli` is a command line tool for interacting with xiso files.

If cargo is set up correctly in the path, it can be installed with:

```sh
$ cargo install xdvdfs-cli
```

Otherwise, it can be run from the workspace root as the default project.

A binary distribution of xdvdfs-cli is also available in the github releases.

### Usage

Running `xdvdfs` with no args will bring up the help screen, showing supported subcommands:

```
Usage: xdvdfs [COMMAND]

Commands:
  ls           List files in an image
  tree         List all files in an image, recursively
  md5          Show MD5 checksums for files in an image
  checksum     Compute deterministic checksum of image contents
  info         Print information about image metadata
  copy-out     Copy a file or directory out of the provided image file
  unpack       Unpack an entire image to a directory
  pack         Pack an image from a given directory or source ISO image
  build-image  Pack an image from a given specification
  image-spec   Manage image spec `xdvdfs.toml` files
  compress     Pack and compress an image from a given directory or source ISO image
  help         Print this message or the help of the given subcommand(s)
```

Running a subcommand with the `-h` flag will show help information for that specific subcommand.

#### Packing an Image

To pack an image from a directory, run:

```sh
$ xdvdfs pack <directory> [optional output path]
```

This will create an iso that matches 1-to-1 with the input directory.

#### Repacking an Image

Images can be repacked from an existing ISO image:

```sh
$ xdvdfs pack <input-image> [optional output path]
```

This will create an iso that matches 1-to-1 with the input image.

#### Packing an Image with Path Rewriting

Images can be packed while rewriting host paths to different destinations in the underlying image using the `xdvdfs build-image` subcommand.

If the path remapping functionality is not needed (i.e. you just want a `/**:/{1}` rule)
then you should prefer `xdvdfs pack` instead.

The primary method of accomplishing this is with a `xdvdfs.toml` file:

```toml
[metadata]

# Relative path to output iso, if not specified in command [optional]
output = "dist/image.xiso.iso"

# List of host-to-image path mapping rules. At least one rule is required.
# All paths are relative to the provided source path, the `xdvdfs.toml` file,
# or the working directory, in that priority order
# Host paths are matched by glob pattern
# Image paths have fields given by `{x}` substituted, where `x` is the index
# of the glob match, starting at 1. `{0}` matches the entire host path.
# Globs are evaluated in the provided order
[map_rules]

# Map contents of the "bin" directory to the image root
bin = "/"

# Map anything in the assets directory to `/assets/`
# Equivalent to `assets = "/assets"`
"assets/**" = "/assets/{1}"

# Map any file in the `sound` subdirectory with name `priority`
# and any extension to the same path in the image
# Note that `{0}` matches the entire relative host path
# Also note that due to the linear ordering of glob matches,
# this takes precedence over the below rule
"sound/priority.*" = "/{0}"

# Map any file in the `sound` subdirectory with extension `a`, `b`, or `c`,
# to `/a/filename`, "/b/filename" or `/c/filename`, based on its filename
# and extension.
"sound/*.{a,b,c}" = "/{2}/{1}"

# but, exclude any files in the `sound` subdirectory with filename `excluded`
# The image path is a don't-care value, and has no effect
"!sound/excluded.*" = ""

# Since globs are evaluated in order, this includes any otherwise excluded
# files in the `sound` subdirectory with name `excluded` and extension `c`
"sound/excluded.c" = "/c/excluded"
```

Assuming `xdvdfs.toml` and all of the above paths are relative to the current directory, the image can be packed with:

```sh
# Produces `dist/image.xiso.iso` with the above configuration
$ xdvdfs build-image
```

There are other ways to pack the image from other directories:

```sh
# Produces `<path-to-source-dir>/dist/image.xiso.iso`
$ xdvdfs build-image <path-to-source-dir>

# Also produces `<path-to-source-dir>/dist/image.xiso.iso`
$ xdvdfs build-image <path-to-source-dir>/xdvdfs.toml

# Produces `./dist/output.xiso.iso` in the current directory
$ xdvdfs build-image <path-to-source-dir> dist/output.xiso.iso

# Produces `<path-to-source-dir>/dist/image.xiso.iso`, with `xdvdfs.toml` not
# necessarily being in `<path-to-source-dir>. Here it is in the current directory
$ xdvdfs build-image -f xdvdfs.toml <path-to-source-dir>
```

To see what the real mapping is given an `xdvdfs.toml` without actually
packing the image, use the `-D` or `--dry-run` flag.

It is also possible to provide all the configuration of an `xdvdfs.toml` file
to `build-image` in the command line directly.

- Use `-O <path>` to supply the `output` field
- Use `-m <host-glob>:<image-path>` to provide a map rule. This can be repeated, and match in the order given.

These can also be combined with `--dry-run` to test different mappings.

To convert a set of command line options to `build-image` into an `xdvdfs.toml` file,
use the `xdvdfs image-spec from` command with the same arguments.

```sh
# Outputs equivalent `xdvdfs.toml` to stdout
$ xdvdfs image-spec from -O dist/image.iso -m "bin:/" -m "assets:/{0}"

# Outputs equivalent `xdvdfs.toml` to a file
$ xdvdfs image-spec from -O dist/image.iso -m "bin:/" -m "assets:/{0}" xdvdfs.toml
```

The generated spec file can then be used with `build-image`.

#### Unpacking

To unpack an image, run:

```sh
$ xdvdfs unpack <path to image> [optional output path]
```

#### Other Utilities

`xdvdfs-cli` supports additional utility tools for use with images.

| Command | Action |
| - | - |
| `xdvsfs ls <path to image> [path within image]` | Lists files within the specified directory, defaulting to root |
| `xdvdfs tree <path to image>` | Prints a listing of every file within the image |
| `xdvdfs md5 <path to image> [optional path to file within image]` | Prints md5 sums for specified files, or every file, within the image |
| `xdvdfs checksum [path to img1]...` | Computes a checksum for all image contents to check integrity against other images |
| `xdvdfs info <path to image> [path within image]` | Prints metadata info for the specified directory entry, or root volume |
| `xdvdfs copy-out <path to image> <path within image> <destination path>` | Copies a single file or directory out of the provided image |

## xdvdfs-core

`xdvdfs-core` is a library for working with XDVDFS metadata.

A simple example reading a file from a given path is:

```rust
async fn read_from_path(xiso: &Path, file_path: &str) -> Box<[u8]> {
    let mut xiso = std::fs::File::open(xiso).unwrap();
    let volume = xdvdfs::read::read_volume(&mut xiso).await.unwrap();

    let file_dirent = volume.root_table.walk_path(&mut xiso, file_path).await.unwrap();

    let data = file_dirent.node.dirent.read_data_all(&mut xiso).await.unwrap();
    data
}
```

This library supports no_std. Custom block devices can be defined by implementing the traits in `xdvdfs::blockdev`.

Without the `alloc` feature, only basic metadata features are supported. The `alloc` feature enables several utility
functions that require allocation (such as `read_data_all` above.

The source code for xdvdfs-cli provides a more detailed example of how to use xdvdfs-core in an environment with std.

Note that xdvdfs is currently not API stable, and following semver with major version 0, each minor version bump may or
may not include breaking changes.
