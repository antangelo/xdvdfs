use std::time::SystemTime;

use anyhow::bail;
use nfsserve::{
    nfs::{fattr3, fileid3, filename3, nfspath3, nfsstat3, nfstime3, sattr3},
    tcp::{NFSTcp, NFSTcpListener},
    vfs::{NFSFileSystem, ReadDirResult, ReadDirSimpleResult},
};

use super::FSMounter;

pub struct NFSFilesystem<F: super::Filesystem> {
    fs: F,
}

impl<F: super::Filesystem> NFSFilesystem<F> {
    pub fn new(fs: F) -> Self {
        Self { fs }
    }
}

fn systime_to_nfstime(time: &SystemTime) -> Result<nfstime3, nfsstat3> {
    let duration = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .inspect_err(|e| log::error!("[systime_to_nfsstime] Error converting time: {e}"))
        .map_err(|_| nfsstat3::NFS3ERR_SERVERFAULT)?;
    let seconds: u32 = duration
        .as_secs()
        .try_into()
        .inspect_err(|e| log::error!("[systime_to_nfsstime] Error truncating seconds to u32: {e}"))
        .map_err(|_| nfsstat3::NFS3ERR_SERVERFAULT)?;
    Ok(nfstime3 {
        seconds,
        nseconds: duration.subsec_nanos(),
    })
}

fn nfs_attr(attr: &super::FileAttribute) -> Result<fattr3, nfsstat3> {
    let ftype = if attr.is_dir {
        nfsserve::nfs::ftype3::NF3DIR
    } else {
        nfsserve::nfs::ftype3::NF3REG
    };

    let mode = if attr.is_writeable {
        0o666 // rw-rw-rw-
    } else {
        0o444 // r--r--r--
    };

    Ok(fattr3 {
        ftype,
        mode,
        nlink: 1,
        uid: 507,
        gid: 507,
        size: attr.byte_size,
        used: attr.block_size,
        rdev: nfsserve::nfs::specdata3::default(),
        fsid: 0,
        fileid: attr.inode,
        atime: systime_to_nfstime(&attr.atime)?,
        mtime: systime_to_nfstime(&attr.mtime)?,
        ctime: systime_to_nfstime(&attr.ctime)?,
    })
}

impl super::FilesystemError {
    fn to_nfsstat3(&self) -> nfsstat3 {
        use super::FilesystemErrorKind;
        match self.kind {
            FilesystemErrorKind::NotImplemented => nfsstat3::NFS3ERR_NOTSUPP,
            FilesystemErrorKind::IOError => nfsstat3::NFS3ERR_IO,
            FilesystemErrorKind::NotDirectory => nfsstat3::NFS3ERR_NOTDIR,
            FilesystemErrorKind::IsDirectory => nfsstat3::NFS3ERR_ISDIR,
            FilesystemErrorKind::NoEntry => nfsstat3::NFS3ERR_NOENT,
        }
    }
}

struct NFSReadDirFiller {
    entries: Vec<(u64, String)>,
    start_after: u64,
    max_entries: u64,
    found_start_after: bool,
}

impl NFSReadDirFiller {
    fn new(start_after: u64, max_entries: u64) -> Self {
        Self {
            entries: Vec::new(),
            start_after,
            max_entries,

            // start_after == 0 <=> include all entries
            found_start_after: start_after == 0,
        }
    }
}

impl super::ReadDirFiller for NFSReadDirFiller {
    fn add(&mut self, inode: u64, _is_dir: bool, name: &str) -> bool {
        if !self.found_start_after {
            if inode == self.start_after {
                self.found_start_after = true;
            }

            return false;
        }

        self.entries.push((inode, name.to_string()));

        self.max_entries -= 1;
        self.max_entries == 0
    }
}

#[async_trait::async_trait]
impl<F: super::Filesystem + Send + Sync> NFSFileSystem for NFSFilesystem<F> {
    fn capabilities(&self) -> nfsserve::vfs::VFSCapabilities {
        nfsserve::vfs::VFSCapabilities::ReadOnly
    }

    fn root_dir(&self) -> fileid3 {
        1
    }

    async fn lookup(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        log::info!(
            "[lookup dirid={dirid} name={}]",
            str::from_utf8(filename).unwrap_or("<invalid-name>")
        );
        let filename = str::from_utf8(filename)
            .inspect_err(|e| log::error!("[lookup] Error parsing UTF-8 filename: {e}"))
            .map_err(|_| nfsstat3::NFS3ERR_NOENT)?;
        self.fs
            .lookup(dirid, filename)
            .await
            .inspect_err(|e| log::error!("[lookup {dirid}] Error {e}"))
            .map_err(|e| e.to_nfsstat3())
            .map(|attr| attr.inode)
    }

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        log::info!("[getattr id={id}]");
        let attr = self
            .fs
            .getattr(id)
            .await
            .inspect_err(|e| log::error!("[getattr {id}] Error {e}"))
            .map_err(|e| e.to_nfsstat3())?;
        nfs_attr(&attr)
            .inspect_err(|e| log::error!("[getattr {id}] nfs_attr error {e:?}"))
            .inspect(|attr| log::trace!("[getattr {id}] attr: {attr:?}"))
    }

    async fn setattr(&self, _id: fileid3, _setattr: sattr3) -> Result<fattr3, nfsstat3> {
        // FIXME: Add write ops to super::Filesystem
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn read(
        &self,
        id: fileid3,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        log::info!("[read id={id} offset={offset} count={count}]");
        self.fs
            .read(id, offset, count as u64)
            .await
            .inspect_err(|e| log::error!("[read {id} {offset} {count}] Error {e}"))
            .map_err(|e| e.to_nfsstat3())
    }

    async fn write(&self, _id: fileid3, _offset: u64, _data: &[u8]) -> Result<fattr3, nfsstat3> {
        // FIXME: Add write ops to super::Filesystem
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn create(
        &self,
        _dirid: fileid3,
        _filename: &filename3,
        _attr: sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        // FIXME: Add write ops to super::Filesystem
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn create_exclusive(
        &self,
        _dirid: fileid3,
        _filename: &filename3,
    ) -> Result<fileid3, nfsstat3> {
        // FIXME: Add write ops to super::Filesystem
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn mkdir(
        &self,
        _dirid: fileid3,
        _dirname: &filename3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        // FIXME: Add write ops to super::Filesystem
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn remove(&self, _dirid: fileid3, _filename: &filename3) -> Result<(), nfsstat3> {
        // FIXME: Add write ops to super::Filesystem
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn rename(
        &self,
        _from_dirid: fileid3,
        _from_filename: &filename3,
        _to_dirid: fileid3,
        _to_filename: &filename3,
    ) -> Result<(), nfsstat3> {
        // FIXME: Add write ops to super::Filesystem
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn readdir(
        &self,
        dirid: fileid3,
        start_after: fileid3,
        max_entries: usize,
    ) -> Result<ReadDirResult, nfsstat3> {
        use nfsserve::vfs::DirEntry;
        log::info!("[readdir dirid={dirid} start_after={start_after} max_entries={max_entries}]");

        // start_after is specified as an inode, so we must start at
        // offset 0 to determine which inode to start including
        let mut filler = NFSReadDirFiller::new(start_after, max_entries as u64);
        let end = self
            .fs
            .readdir(dirid, /*offset=*/ 0, &mut filler)
            .await
            .inspect_err(|e| log::error!("[readdir {dirid}] Error {e}"))
            .map_err(|e| e.to_nfsstat3())?;

        // readdir requires that attributes are included in the response
        let mut entries: Vec<DirEntry> = Vec::new();
        entries.reserve_exact(filler.entries.len());
        for (fileid, name) in filler.entries {
            let attr = self
                .fs
                .getattr(fileid)
                .await
                .inspect_err(|e| log::error!("[readdir attr {fileid}] Error {e}"))
                .map_err(|e| e.to_nfsstat3())?;
            let attr = nfs_attr(&attr)
                .inspect_err(|e| log::error!("[readdir attr {fileid}] nfs_attr error {e:?}"))?;
            let name: nfsserve::nfs::nfsstring = name.into_bytes().into();
            entries.push(DirEntry { fileid, name, attr });
        }

        Ok(ReadDirResult { entries, end })
    }

    async fn readdir_simple(
        &self,
        dirid: fileid3,
        start_after: fileid3,
        max_entries: usize,
    ) -> Result<ReadDirSimpleResult, nfsstat3> {
        use nfsserve::vfs::DirEntrySimple;
        log::info!(
            "[readdir_simple dirid={dirid} start_after={start_after} max_entries={max_entries}]"
        );

        // start_after is specified as an inode, so we must start at
        // offset 0 to determine which inode to start including
        let mut filler = NFSReadDirFiller::new(start_after, max_entries as u64);
        let end = self
            .fs
            .readdir(dirid, /*offset=*/ 0, &mut filler)
            .await
            .inspect_err(|e| log::error!("[readdir_simple {dirid}] Error {e}"))
            .map_err(|e| e.to_nfsstat3())?;

        let entries: Vec<DirEntrySimple> = filler
            .entries
            .into_iter()
            .map(|(fileid, name)| DirEntrySimple {
                fileid,
                name: name.into_bytes().into(),
            })
            .collect();

        Ok(ReadDirSimpleResult { entries, end })
    }

    async fn symlink(
        &self,
        _dirid: fileid3,
        _linkname: &filename3,
        _symlink: &nfspath3,
        _attr: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        // FIXME: Add symlink support to super::Filesystem
        // No common FS planned will need them, though,
        // so this would be for completeness
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    async fn readlink(&self, _id: fileid3) -> Result<nfspath3, nfsstat3> {
        // FIXME: Add symlink support to super::Filesystem
        // No common FS planned will need them, though,
        // so this would be for completeness
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }
}

pub struct NFSMounter {
    port: u16,
}

impl Default for NFSMounter {
    fn default() -> Self {
        Self { port: 11111 }
    }
}

impl FSMounter for NFSMounter {
    fn process_args(
        &mut self,
        _mount_point: Option<&std::path::Path>,
        _src: &std::path::Path,
        options: &[String],
    ) -> anyhow::Result<super::TopLevelOptions> {
        for opt in options {
            match opt {
                x if x.starts_with("port=") => {
                    self.port = x[5..].parse()?;
                }
                x => bail!("Unsupported mount option {x}"),
            }
        }

        // TODO: Support forking with NFS filesystems
        // There is no way to exit the daemon if forked currently
        Ok(super::TopLevelOptions { fork: false })
    }

    fn mount<F: super::Filesystem + 'static>(
        self,
        fs: F,
        rt: &tokio::runtime::Runtime,
        mount_point: Option<&std::path::Path>,
    ) -> anyhow::Result<()> {
        let nfs = NFSFilesystem::new(fs);

        let mount_point_string = match mount_point {
            Some(mount_point) => mount_point.display().to_string(),
            None => "<mount point>".to_string(),
        };

        rt.block_on(async move {
            let listener = NFSTcpListener::bind(&format!("127.0.0.1:{}", self.port), nfs).await?;

            println!("NFS server listening on port {}", self.port);

            // FIXME: Support mount hints for other operating systems
            println!("Mount with (may require root):");
            println!(
                "mount -t nfs -o user,noacl,nolock,vers=3,tcp,wsize=1048576,rsize=131072,actimeo=120,port={},mountport={} localhost:/ {mount_point_string}",
                self.port, self.port
                );

            listener.handle_forever().await?;
            Ok(())
        })
    }
}
