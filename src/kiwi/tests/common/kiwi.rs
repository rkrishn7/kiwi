use std::io::Write;

use anyhow::Context;
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use tempfile::NamedTempFile;

pub struct Process {
    proc: std::process::Child,
}

impl Process {
    pub fn new_with_args(args: &[&str]) -> anyhow::Result<Self> {
        let proc = std::process::Command::new("kiwi")
            .args(args)
            .spawn()
            .context("failed to spawn kiwi process")?;

        Ok(Self { proc })
    }

    pub fn kill(&mut self) {
        self.proc.kill().expect("failed to kill kiwi process");
    }

    pub fn signal(&mut self, signal: Signal) {
        signal::kill(Pid::from_raw(self.proc.id().try_into().unwrap()), signal)
            .expect("failed to send signal to kiwi process");
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.kill();
    }
}

pub struct ConfigFile {
    inner: NamedTempFile,
}

impl ConfigFile {
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        let mut file = NamedTempFile::new().context("failed to create temporary file")?;
        file.as_file_mut()
            .write_all(s.as_bytes())
            .context("failed to write config to temporary file")?;

        Ok(Self { inner: file })
    }

    pub fn as_file(&self) -> &std::fs::File {
        self.inner.as_file()
    }

    pub fn as_file_mut(&mut self) -> &std::fs::File {
        self.inner.as_file_mut()
    }

    pub fn path(&self) -> &std::path::Path {
        self.inner.path()
    }

    pub fn path_str(&self) -> &str {
        self.path().to_str().expect("path is not valid utf-8")
    }
}
