# xdvdfs-cli

`xdvdfs-cli` is a command line tool for interacting with xiso files.

If cargo is set up correctly in the path, it can be installed with:

```sh
$ cargo install xdvdfs-cli
```

Otherwise, it can be run from the workspace root as the default project.

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
  compress  Pack and compress an image from a given directory or source ISO image
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
