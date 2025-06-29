# xdvdfs-fsd

`xdvdfs-fsd` is a host filesystem daemon for mounting xdvdfs image and virtual pack-overlay filesystems.

Mounting is supported over FUSE (for platforms that support it) or through a local NFS server.

## Installation

`xdvdfs-fsd` is not built by default or shipped as a crate for download, so it must be built from source.

```sh
git clone https://github.com/antangelo/xdvdfs.git
cd xdvdfs
cargo install --path xdvdfs-fsd
```

This will install the `xdvdfsd` binary. Root is not required to run the binary,
but installing it system-wide may be preferable for use with `mount`.

## Usage

`xdvdfsd` has two operating modes, determined by whether the source input is a file or directory.

To mount a filesystem in either mode, see [Mounting the Filesystem](#mounting-the-filesystem).

### Image Mode

Supplying an XDVDFS image file to xdvdfsd will mount the image contents at the mountpoint.

The image contents are read-only, and allows for reading or copying data out of an XDVDFS image.
Image mode supports XISO format images, as well as any of the XGD formats.

Note that modifying the underlying image file will result in unspecified behavior in
the mounted filesystem, and may result in data corruption in the mounted filesystem.

### Pack-Overlay Mode

Supplying a directory to xdvdfsd will mount the directory in pack-overlay mode.
The contents of the mounted filesystem are determined as follows:

1. Any files or directories in the underlying filesystem are passed through (but are read-only in the mounted filesystem).
1. Any file with name `<filename>.iso` or `<filename>.xiso` that contains an XDVDFS image will create a directory `<filename>`,
containing the read-only contents of the XDVDFS image (as if it were mounted in image mode).
1. Any directory `<dirname>` containing `default.xbe` will create a file `<dirname>.xiso`, an XISO image with the contents of `<dirname>`.

The files and directories that appear in the mounted filesystem do not exist on disk and do not occupy disk space (other than space used by
the underlying filesystem). Image packing is done on-demand and the necessary metadata is stored in-memory, with file contents fetched from
the underlying filesystem as needed.

Note that modifying the underlying filesystem can result in unspecified behavior in the mounted filesystem, and reads to files in the overlay
may return corrupted data.

## Mounting the Filesystem

### Mount Using FUSE (Linux only)

`xdvdfsd` supports `mount`-like arguments.

```sh
xdvdfsd path/to/source /mnt/mountpoint
mount -t fuse.xdvdfsd path/to/source /mnt/mountpoint
```

Options can be specified with `-o`, similar to other filesystems. All fuse options are supported.

`fork` and `nofork` can be specified to run as a daemon (default is to fork).

The filesystem can be unmounted through `fusermount` or `umount`.

#### Using the `mount` Command

For use with `mount`, symlink to `/sbin/mount.xdvdfs`:

```sh
sudo ln -s `which xdvdfsd` /sbin/mount.xdvdfs
```

Then, it can be directly used as a mount type:

```sh
mount -t xdvdfs path/to/source /mnt/mountpoint
```

## Mount using NFS

For systems that do not support FUSE, a local NFS server can be used as an alternative.

```sh
xdvdfsd -b nfs path/to/source
```

This will start an NFS server on port `11111` by default. The port can be changed by specifying `-o port=<PORT>`.

A command will be printed to stdout to mount the NFS server. Root may be required for the mount command depending on
the operating system, but xdvdfsd does not require root access.
