#[cfg(unix)]
type Inner = std::os::fd::OwnedFd;

#[cfg(not(unix))]
type Inner = ();

pub struct Daemonize(Inner, bool);

impl Drop for Daemonize {
    fn drop(&mut self) {
        if !self.1 {
            self.finish_impl().unwrap();
        }
    }
}

impl Daemonize {
    pub fn finish(mut self) -> anyhow::Result<()> {
        if self.1 {
            return Ok(());
        }

        self.finish_impl()?;
        self.1 = true;
        Ok(())
    }
}

#[cfg(unix)]
impl Daemonize {
    /// Fork the daemon off of the parent
    /// This function will never return in the parent,
    /// and will return a Daemonize handle to the child.
    /// The parent will wait for the child to drop, or
    /// to call `finish()` before exiting.
    /// Safety: Unsafe when called in multithreaded context.
    pub unsafe fn fork() -> anyhow::Result<Daemonize> {
        use nix::unistd;
        use std::os::fd::AsRawFd;
        let (r, w) = unistd::pipe()?;

        // Safety: fork is unsafe in multithreaded
        // programs, guaranteed by caller.
        let fork = unsafe { unistd::fork()? };
        match fork {
            unistd::ForkResult::Parent { child: _ } => {
                // Wait for message on pipe, then exit
                std::mem::drop(w);
                let mut buf = [0];
                unistd::read(r.as_raw_fd(), &mut buf)?;
                std::mem::drop(r);
                std::process::exit(0);
            }
            unistd::ForkResult::Child => {
                std::mem::drop(r);
                Ok(Daemonize(w, false))
            }
        }
    }

    fn finish_impl(&mut self) -> anyhow::Result<()> {
        nix::unistd::write(&self.0, &[0])?;
        Ok(())
    }
}

#[cfg(not(unix))]
impl Daemonize {
    /// Daemonizing is not supported on this platform,
    /// all functions are no-op.
    pub unsafe fn fork() -> anyhow::Result<Daemonize> {
        Ok(Daemonize((), true))
    }

    fn finish_impl(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
