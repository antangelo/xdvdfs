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
  ls      List files in an image
  tree    List all files in an image, recursively
  md5     Show MD5 checksums for files in an image
  unpack  Unpack an entire image to a directory
  pack    Pack an image from a given directory
  help    Print this message or the help of the given subcommand(s)
```

Running a subcommand with the `-h` flag will show help information for that specific subcommand.
