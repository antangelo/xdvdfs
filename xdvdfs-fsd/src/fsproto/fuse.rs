use std::{path::Path, time::Duration};

use anyhow::bail;
use fuser::MountOption;
use log::{error, info};
use tokio::runtime::Runtime;

use super::{FSMounter, FileAttribute, TopLevelOptions};

pub struct FuseFilesystem<'a, F: super::Filesystem> {
    fs: F,
    rt: &'a Runtime,
}

impl<'a, F: super::Filesystem> FuseFilesystem<'a, F> {
    pub fn new(fs: F, rt: &'a Runtime) -> Self {
        Self { fs, rt }
    }
}

fn fuse_attr(req: &fuser::Request<'_>, attr: &FileAttribute) -> fuser::FileAttr {
    let kind = if attr.is_dir {
        fuser::FileType::Directory
    } else {
        fuser::FileType::RegularFile
    };

    let perm = if attr.is_writeable {
        0o666 // rw-rw-rw-
    } else {
        0o444 // r--r--r--
    };

    fuser::FileAttr {
        ino: attr.inode,
        size: attr.byte_size,
        blocks: attr.byte_size.div_ceil(attr.block_size),
        atime: attr.atime,
        mtime: attr.mtime,
        ctime: attr.ctime,
        crtime: attr.crtime,
        kind,
        perm,
        nlink: 0,
        uid: req.uid(),
        gid: req.gid(),
        rdev: 0,
        blksize: attr.block_size as u32,
        flags: 0,
    }
}

impl super::FilesystemError {
    fn to_libc(&self) -> libc::c_int {
        use super::FilesystemErrorKind;
        match self.kind {
            FilesystemErrorKind::NotDirectory => libc::ENOTDIR,
            FilesystemErrorKind::IsDirectory => libc::EISDIR,
            FilesystemErrorKind::NoEntry => libc::ENOENT,
            FilesystemErrorKind::IOError => libc::EIO,
            _ => libc::ENOTSUP,
        }
    }
}

struct ReplyDirectoryFiller<'a> {
    reply: &'a mut fuser::ReplyDirectory,
    idx: i64,
}

impl<'a> super::ReadDirFiller for ReplyDirectoryFiller<'a> {
    fn add(&mut self, inode: u64, is_dir: bool, name: &str) -> bool {
        self.idx += 1;
        let file_type = if is_dir {
            fuser::FileType::Directory
        } else {
            fuser::FileType::RegularFile
        };

        info!(
            "[readdir] push record (inode = {inode}, idx = {0}, name = {name})",
            self.idx
        );
        self.reply.add(inode, self.idx, file_type, name)
    }
}

impl<'a, F: super::Filesystem> fuser::Filesystem for FuseFilesystem<'a, F> {
    fn lookup(
        &mut self,
        req: &fuser::Request<'_>,
        parent: u64,
        name: &std::ffi::OsStr,
        reply: fuser::ReplyEntry,
    ) {
        info!("[lookup] For {name:?} in inode {parent}");

        let Some(name) = name.to_str() else {
            reply.error(libc::EINVAL);
            return;
        };

        let res = self.fs.lookup(parent, name);
        let res = self
            .rt
            .block_on(res)
            .inspect_err(|e| error!("[lookup parent={parent} name={name:?}] error: {e}"));
        match &res {
            Ok(attr) => reply.entry(&Duration::new(0, 0), &fuse_attr(req, attr), 0),
            Err(err) => reply.error(err.to_libc()),
        }
    }

    fn getattr(
        &mut self,
        req: &fuser::Request<'_>,
        ino: u64,
        _fh: Option<u64>,
        reply: fuser::ReplyAttr,
    ) {
        info!("[getattr ino={ino}]");

        let attr = self.fs.getattr(ino);
        let attr = self
            .rt
            .block_on(attr)
            .inspect_err(|e| error!("[getattr ino={ino}] error: {e}"));
        match &attr {
            Ok(attr) => reply.attr(&Duration::new(0, 0), &fuse_attr(req, attr)),
            Err(err) => reply.error(err.to_libc()),
        }
    }

    fn open(&mut self, _req: &fuser::Request<'_>, ino: u64, flags: i32, reply: fuser::ReplyOpen) {
        info!("[open] for inode {ino}");

        let is_writeable = self.fs.is_writeable(ino);
        let is_writeable = self.rt.block_on(is_writeable);
        let is_writeable = match is_writeable {
            Ok(x) => x,
            Err(err) => {
                error!("[open ino={ino}] error {err}");
                reply.error(err.to_libc());
                return;
            }
        };

        if is_writeable {
            let unsupported_flags = libc::O_WRONLY | libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC;
            if flags & unsupported_flags != 0 {
                error!(
                    "[open ino={ino}] Unsupported flag {}",
                    flags & unsupported_flags
                );
                reply.error(libc::ENOTSUP);
                return;
            }
        }

        reply.opened(0, 0);
    }

    fn read(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyData,
    ) {
        info!("[read ino={ino} offset={offset} size={size}]");

        if offset < 0 {
            reply.error(libc::EINVAL);
            return;
        }

        let res = self.fs.read(ino, offset as u64, size as u64);
        let res = self
            .rt
            .block_on(res)
            .inspect_err(|e| error!("[read ino={ino} offset={offset} size={size}] error: {e}"));
        match res {
            Ok((bytes, _)) => reply.data(bytes.as_ref()),
            Err(err) => reply.error(err.to_libc()),
        }
    }

    fn opendir(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        _flags: i32,
        reply: fuser::ReplyOpen,
    ) {
        info!("[opendir ino={ino}]");
        reply.opened(0, 0);
    }

    fn readdir(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectory,
    ) {
        info!("[readdir ino={ino} offset={offset}]");

        if offset < 0 {
            error!("[readdir ino={ino} offset={offset}] Invalid offset {offset}");
            reply.error(libc::EINVAL);
            return;
        }

        // add's offset is 1-indexed
        let mut idx = offset;
        if offset == 0 {
            if reply.add(ino, idx + 1, fuser::FileType::Directory, ".") {
                return;
            }

            idx += 1;
        }

        if offset <= 1 {
            if reply.add(ino, idx + 1, fuser::FileType::Directory, "..") {
                return;
            }

            idx += 1;
        }

        info!("[readdir] starting at index {idx}");
        let mut filler = ReplyDirectoryFiller {
            reply: &mut reply,
            idx,
        };

        // Adjust offset by the '.' and '..' records
        let offset = match offset {
            // This call emits '.' and '..', guest starts at idx 0
            0 => 0,
            // This call emits '..', prior call emits '.'. Guest starts at 0
            1 => 0,
            // Prior call emitted '.' and '..'. Guest starts at 2 less than indicated
            offset => offset - 2,
        };

        let res = self.fs.readdir(ino, offset as u64, &mut filler);
        let res = self
            .rt
            .block_on(res)
            .inspect_err(|e| error!("[readdir ino={ino} offset={offset}] error: {e}"));
        match res {
            Ok(_) => reply.ok(),
            Err(err) => reply.error(err.to_libc()),
        }
    }

    fn access(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        _mask: i32,
        reply: fuser::ReplyEmpty,
    ) {
        info!("[access ino={ino}]");
        reply.ok();
    }
}

#[derive(Default)]
pub struct FuseFSMounter {
    mount_opts: Vec<MountOption>,
}

impl FSMounter for FuseFSMounter {
    fn process_args(
        &mut self,
        mount_point: Option<&Path>,
        src: &Path,
        opts: &[String],
    ) -> anyhow::Result<TopLevelOptions> {
        if mount_point.is_none() {
            bail!("Mount point must be specified for FUSE mounting");
        };

        self.mount_opts.clear();
        self.mount_opts.reserve_exact(opts.len());

        let mut has_fsname = false;
        let mut has_subtype = false;
        let mut tlo = TopLevelOptions { fork: true };

        for opt in opts {
            for opt in opt.split(",") {
                let opt = match opt {
                    "auto_unmount" => MountOption::AutoUnmount,
                    "allow_other" => MountOption::AllowOther,
                    "allow_root" => MountOption::AllowRoot,
                    "default_permissions" => MountOption::DefaultPermissions,
                    "suid" => MountOption::Suid,
                    "nosuid" => MountOption::NoSuid,
                    "ro" => MountOption::RO,
                    "rw" => MountOption::RW,
                    "exec" => MountOption::Exec,
                    "noexec" => MountOption::NoExec,
                    "dev" => MountOption::Dev,
                    "nodev" => MountOption::NoDev,
                    x if x.starts_with("fsname=") => {
                        has_fsname = true;
                        MountOption::FSName(x[7..].into())
                    }
                    x if x.starts_with("subtype=") => {
                        has_subtype = true;
                        MountOption::Subtype(x[8..].into())
                    }
                    "fork" => {
                        tlo.fork = true;
                        continue;
                    }
                    "nofork" => {
                        tlo.fork = false;
                        continue;
                    }
                    x => bail!("Unsupported mount option {x}"),
                };

                self.mount_opts.push(opt);
            }
        }

        if !has_fsname {
            self.mount_opts
                .push(MountOption::FSName(src.to_string_lossy().to_string()));
        }

        if !has_subtype {
            self.mount_opts
                .push(MountOption::Subtype("xdvdfs".to_string()));
        }

        Ok(tlo)
    }

    fn mount<F: crate::fsproto::Filesystem + 'static>(
        self,
        fs: F,
        rt: &tokio::runtime::Runtime,
        mount_point: Option<&Path>,
    ) -> anyhow::Result<()> {
        let fs = FuseFilesystem::new(fs, rt);
        let mount_point = mount_point.expect("mount_point should be checked in process_args");
        fuser::mount2(fs, mount_point, &self.mount_opts)?;
        Ok(())
    }
}
