# xdvdfs-fsd

`xdvdfs-fsd` is a host filesystem daemon, using FUSE, for mounting xdvdfs images.

## Installation

Currently, `xdvdfs-fsd` is only supported on Linux systems (via FUSE). Other systems with FUSE capability may or may not work, but are not tested.

`xdvdfs-fsd` is not built by default or shipped as a crate for download, so it must be built from source.

```sh
$ git clone git@github.com:antangelo/xdvdfs.git
$ cd xdvdfs
$ cargo install --path xdvdfs-fsd
```

This will install the `xdvdfsd` binary. Root is not required to run the binary,
but installing it system-wide may be preferable for use with `mount`.

### Use With `mount`

For use with `mount`, symlink to `/sbin/mount.xdvdfs`:

```sh
$ sudo ln -s `which xdvdfsd` /sbin/mount.xdvdfs
```

## Usage

`xdvdfsd` supports `mount`-like arguments.

```sh
$ xdvdfsd path/to/image.iso /mnt/mountpoint
$ mount -t fuse.xdvdfsd path/to/image.iso /mnt/mountpoint
```

If installed for use with `mount`:

```sh
$ mount -t xdvdfs path/to/image.iso /mnt/mountpoint
```

Options can be specified with `-o`, similar to other filesystems.

All fuse options are supported.
`fork` and `nofork` can be specified to run as a daemon (default is to fork).

The filesystem can be unmounted through `fusermount` or `umount`.
