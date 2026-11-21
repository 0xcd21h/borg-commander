use super::*;

fn file_with(text: &str) -> OwnedFd {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(text.as_bytes()).unwrap();
    OwnedFd::from(file)
}

fn text_of(file: OwnedFd) -> String {
    let mut file = File::from(file);
    file.rewind().unwrap();
    let mut text = String::new();
    file.read_to_string(&mut text).unwrap();
    text
}

/// What a Borg queues for the next before it starts it, as `exec` does.
fn queue(stream: &UnixStream, stdout: &OwnedFd, carried: &Carried) {
    let record = record_file(&carried.record).unwrap();
    let ahead = [stdout.as_fd(), record.as_fd()];
    send(stream, ahead.into_iter().chain(carried.files.iter().map(AsFd::as_fd))).unwrap();
}

#[test]
fn the_new_borg_finds_its_standard_output_the_description_and_the_open_files_in_order() {
    let count = FILES_PER_MESSAGE * 2 + 3;
    let files = (0..count).map(|index| file_with(&format!("file {index}"))).collect();
    let carried = Carried { record: b"a description".repeat(100_000), files };
    let (ours, theirs) = UnixStream::pair().unwrap();
    queue(&ours, &file_with("the terminal"), &carried);
    drop(ours);
    let (stdout, arrived) = take(&theirs).unwrap();
    assert_eq!(text_of(stdout), "the terminal");
    assert_eq!(arrived.record, carried.record);
    let texts: Vec<_> = arrived.files.into_iter().map(text_of).collect();
    let expected: Vec<_> = (0..count).map(|index| format!("file {index}")).collect();
    assert_eq!(texts, expected);
}

#[test]
fn a_borg_that_carried_nothing_is_no_resume() {
    let (ours, theirs) = UnixStream::pair().unwrap();
    drop(ours);
    assert!(take(&theirs).is_err());
}

#[test]
fn only_a_resume_starts_with_the_arguments_it_ran_with_before() {
    let args: Vec<OsString> = [RESUME, "--session", "web"].map(OsString::from).into();
    assert_eq!(split_resume(&args).unwrap(), ["--session", "web"].map(OsString::from));
    assert!(split_resume(&["--session", "web"].map(OsString::from)).is_none());
}
