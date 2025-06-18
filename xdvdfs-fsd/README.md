# xdvdfs-fsd

`xdvdfs-fsd` is a host filesystem daemon for mounting xdvdfs images.

Mounting is supported over FUSE (for platforms that support it) or through a local NFS server.

## Installation

`xdvdfs-fsd` is not built by default or shipped as a crate for download, so it must be built from source.

```sh
$ git clone git@github.com:antangelo/xdvdfs.git
$ cd xdvdfs
$ cargo install --path xdvdfs-fsd
```

This will install the `xdvdfsd` binary. Root is not required to run the binary,
but installing it system-wide may be preferable for use with `mount`.

## Usage with FUSE (Linux only)

`xdvdfsd` supports `mount`-like arguments.

```sh
$ xdvdfsd path/to/image.iso /mnt/mountpoint
$ mount -t fuse.xdvdfsd path/to/image.iso /mnt/mountpoint
```

Options can be specified with `-o`, similar to other filesystems. All fuse options are supported.

`fork` and `nofork` can be specified to run as a daemon (default is to fork).

The filesystem can be unmounted through `fusermount` or `umount`.

### Use With `mount`

For use with `mount`, symlink to `/sbin/mount.xdvdfs`:

```sh
$ sudo ln -s `which xdvdfsd` /sbin/mount.xdvdfs
```

Then, it can be directly used as a mount type:

```sh
$ mount -t xdvdfs path/to/image.iso /mnt/mountpoint
```

## Usage with NFS

For systems that do not support FUSE, a local NFS server can be used as an alternative.

```sh
$ xdvdfsd -b nfs path/to/image.iso
```

This will start an NFS server on port `11111` by default. The port can be changed by specifying `-o port=<PORT>`.

A command will be printed to stdout to mount the NFS server. Root may be required for the mount command depending on
the operating system, but xdvdfsd does not require root access.
