//! Starting Borg again in its own process from its program file, which an update may have
//! replaced: the process keeps its ID, its terminal and the programs it started. The open
//! files it carries, with a description of them, wait on a socket that stands in for the new
//! Borg's standard output, which the new Borg reads first and then puts back. When the new
//! Borg does not come up, ending it lets go of everything it was handed.

use std::{
    convert::Infallible,
    ffi::OsString,
    fs::File,
    io::{self, IoSlice, IoSliceMut, Read, Seek, Write},
    mem::MaybeUninit,
    os::{
        fd::{AsFd, BorrowedFd, OwnedFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    process::{Command, Stdio},
};

use anyhow::{Context, Result, ensure};
use rustix::{
    io::{FdFlags, fcntl_setfd},
    net::{
        RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, SendAncillaryBuffer, SendAncillaryMessage,
        SendFlags, recvmsg, sendmsg,
    },
    stdio::dup2_stdout,
};

/// The argument a Borg starts again with, followed by the arguments it ran with before.
pub const RESUME: &str = "--resume";
/// The most open files one message carries.
const FILES_PER_MESSAGE: usize = 64;
const FILE_LIMIT: usize = 4096;
const RECORD_LIMIT: u64 = 256 * 1024 * 1024;
const HEADER: usize = 4;
/// The byte each batch of open files travels with.
const BATCH: u8 = 0;
/// The open files that travel ahead of the carried ones: standard output, then the record.
const AHEAD: usize = 2;

/// What a Borg carries into the next: a description it reads back, and the open files the
/// description names by their place in the list.
#[derive(Debug, Default)]
pub struct Carried {
    pub record: Vec<u8>,
    pub files: Vec<OwnedFd>,
}

impl Carried {
    /// Adds an open file to carry, and gives its place in the list.
    pub fn carry(&mut self, file: impl Into<OwnedFd>) -> usize {
        self.files.push(file.into());
        self.files.len() - 1
    }

    /// The description, and the open files to take by their place.
    pub fn into_parts(self) -> (Vec<u8>, Files) {
        (self.record, Files(self.files.into_iter().map(Some).collect()))
    }
}

/// The open files a Borg carried, each taken once by its place in the list.
pub struct Files(Vec<Option<OwnedFd>>);

impl Files {
    pub fn take(&mut self, place: usize) -> Result<OwnedFd> {
        self.0.get_mut(place).and_then(Option::take).context("the previous Borg did not carry this open file")
    }
}

/// The version of Borg the program file holds now, which an update may have replaced since
/// this Borg started.
pub async fn installed_version() -> Option<String> {
    let output = tokio::process::Command::new(crate::program::path().ok()?)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    let printed = String::from_utf8(output.stdout).ok()?;
    printed.trim().strip_prefix(crate::update::VERSION_NAME).map(str::to_owned)
}

/// Starts the program file again in this process with `args`, carrying `carried` into it.
/// It returns only when that fails, and this Borg then still has everything it carried.
pub fn exec(args: &[OsString], carried: &Carried) -> Result<Infallible> {
    let program = crate::program::path()?;
    let (ours, theirs) = UnixStream::pair()?;
    let stdout = io::stdout().as_fd().try_clone_to_owned()?;
    let record = record_file(&carried.record)?;
    let ahead = [stdout.as_fd(), record.as_fd()];
    send(&ours, ahead.into_iter().chain(carried.files.iter().map(AsFd::as_fd)))?;
    io::stdout().flush()?;
    dup2_stdout(&theirs)?;
    let error = Command::new(program).arg(RESUME).args(args).exec();
    dup2_stdout(&stdout)?;
    Err(anyhow::Error::from(error).context(borg_i18n::tr!(relaunch_failed)))
}

/// The description in a file of its own, which no other process can open.
fn record_file(record: &[u8]) -> Result<File> {
    let mut file = tempfile::tempfile()?;
    file.write_all(record)?;
    file.rewind()?;
    Ok(file)
}

/// Takes what the previous Borg carried from standard output, and puts back the standard
/// output it had.
pub fn resume() -> Result<Carried> {
    let (stdout, carried) = take(&UnixStream::from(io::stdout().as_fd().try_clone_to_owned()?))?;
    dup2_stdout(&stdout)?;
    Ok(carried)
}

/// The standard output the previous Borg had, and what it carried, from the stream it left
/// with everything queued and its own end closed.
fn take(stream: &UnixStream) -> Result<(OwnedFd, Carried)> {
    let mut files = receive(stream)?.into_iter();
    let (Some(stdout), Some(record)) = (files.next(), files.next()) else {
        anyhow::bail!("the previous Borg carried no description")
    };
    let mut text = Vec::new();
    File::from(record).take(RECORD_LIMIT).read_to_end(&mut text)?;
    Ok((stdout, Carried { record: text, files: files.collect() }))
}

/// The arguments a Borg ran with before it started again, when it did.
pub fn split_resume(args: &[OsString]) -> Option<&[OsString]> {
    match args {
        [flag, rest @ ..] if flag == RESUME => Some(rest),
        _ => None,
    }
}

fn send<'a>(stream: &UnixStream, files: impl Iterator<Item = BorrowedFd<'a>>) -> Result<()> {
    let files: Vec<_> = files.collect();
    ensure!((AHEAD..=FILE_LIMIT).contains(&files.len()), "too many open files to carry");
    let mut writer = stream;
    writer.write_all(&u32::try_from(files.len())?.to_le_bytes())?;
    for batch in files.chunks(FILES_PER_MESSAGE) {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(FILES_PER_MESSAGE))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        ensure!(control.push(SendAncillaryMessage::ScmRights(batch)), "too many open files in one message");
        let sent = sendmsg(stream, &[IoSlice::new(&[BATCH])], &mut control, SendFlags::empty())?;
        ensure!(sent == 1, "the open files did not go through");
    }
    Ok(())
}

fn receive(stream: &UnixStream) -> Result<Vec<OwnedFd>> {
    let mut files = Vec::new();
    let mut header = [0; HEADER];
    read_with_files(stream, &mut header, &mut files)?;
    let count = usize::try_from(u32::from_le_bytes(header))?;
    ensure!(count <= FILE_LIMIT, "the previous Borg carried too many open files");
    let mut batches = vec![0; count.div_ceil(FILES_PER_MESSAGE)];
    read_with_files(stream, &mut batches, &mut files)?;
    ensure!(files.len() == count, "the previous Borg's open files did not all arrive");
    for file in &files {
        fcntl_setfd(file, FdFlags::CLOEXEC)?;
    }
    Ok(files)
}

/// Fills `buffer` from the stream, keeping the open files that come with it.
fn read_with_files(stream: &UnixStream, buffer: &mut [u8], files: &mut Vec<OwnedFd>) -> Result<()> {
    let mut filled = 0;
    while filled < buffer.len() {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(FILES_PER_MESSAGE))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let received =
            recvmsg(stream, &mut [IoSliceMut::new(&mut buffer[filled..])], &mut control, RecvFlags::empty())?;
        for message in control.drain() {
            if let RecvAncillaryMessage::ScmRights(arrived) = message {
                files.extend(arrived);
            }
        }
        ensure!(!received.flags.contains(ReturnFlags::CTRUNC), "open files were lost on the way");
        ensure!(received.bytes > 0, "the previous Borg ended before it handed everything over");
        filled += received.bytes;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
