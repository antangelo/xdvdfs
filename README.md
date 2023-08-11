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
  ls        List files in an image
  tree      List all files in an image, recursively
  md5       Show MD5 checksums for files in an image
  checksum  Compute deterministic checksum of image contents
  info      Print information about image metadata
  unpack    Unpack an entire image to a directory
  pack      Pack an image from a given directory or source ISO image
  help      Print this message or the help of the given subcommand(s)
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
