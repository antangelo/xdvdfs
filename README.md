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
  ls      List files in an image
  tree    List all files in an image, recursively
  md5     Show MD5 checksums for files in an image
  info    Print information about image metadata
  patch   Patch XBE files in an image to run on any media type
  unpack  Unpack an entire image to a directory
  pack    Pack an image from a given directory
  help    Print this message or the help of the given subcommand(s)
```

Running a subcommand with the `-h` flag will show help information for that specific subcommand.

#### Packing an Image

To pack an image from a directory, run:

```sh
$ xdvdfs pack <directory> [optional output path]
```

Depending on the xbe that was packed, this image may or may not be immediately functional.
Unlike other tools, `xdvdfs pack` creates an iso that matches 1-to-1 with the input directory.

Certain xbe files will thus require an allowed media type patch applied to enable them to run out of the produced
iso image. This can be done with the following command:

```sh
$ xdvdfs patch <path to image>
```

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
| `xdvdfs info <path to image> [path within image]` | Prints metadata info for the specified directory entry, or root volume |

## xdvdfs-core

`xdvdfs-core` is a library for working with XDVDFS metadata.

A simple example reading a file from a given path is:

```rust
fn read_from_path(xiso: &Path, file_path: &str) -> Box<[u8]> {
    let mut xiso = std::fs::File::open(xiso).unwrap();
    let volume = xdvdfs::read::read_volume(&mut xiso).unwrap();

    let file_dirent = volume.root_table.walk_path(&mut xiso, file_path).unwrap();

    let data = file_dirent.node.dirent.read_data_all(&mut xiso).unwrap();
    data
}
```

This library supports no_std. Custom block devices can be defined by implementing the traits in `xdvdfs::blockdev`.

Without the `alloc` feature, only basic metadata features are supported. The `alloc` feature enables several utility
functions that require allocation (such as `read_data_all` above.

The source code for xdvdfs-cli provides a more detailed example of how to use xdvdfs-core in an environment with std.

Note that xdvdfs is currently not API stable, and following semver with major version 0, each minor version bump may or
may not include breaking changes.
