use std::io::{self, BufRead, BufReader, Read, Write};

use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use super::armor::ArmorError;
use super::{FILE_KEY_LEN, Input, NONCE_LEN, read_full};

pub const CHUNK_LEN: usize = 64 * 1024;
const TAG_LEN: usize = 16;
const SEALED_CHUNK_LEN: usize = CHUNK_LEN + TAG_LEN;
const COUNTER_LEN: usize = 11;
const COUNTER_LIMIT: u128 = 1 << (COUNTER_LEN * 8);
const LAST_CHUNK: u8 = 1;
const KEY_LEN: usize = 32;
const PAYLOAD_LABEL: &[u8] = b"payload";

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PayloadError {
    #[error("the file ends before its last chunk")]
    Truncated,
    #[error("a chunk failed authentication")]
    Chunk,
    #[error("the last chunk is empty")]
    EmptyLastChunk,
    #[error("there is data after the last chunk")]
    TrailingData,
    #[error("the file has too many chunks")]
    TooManyChunks,
}

impl From<PayloadError> for io::Error {
    fn from(error: PayloadError) -> Self {
        io::Error::new(io::ErrorKind::InvalidData, error)
    }
}

fn payload_cipher(file_key: &[u8; FILE_KEY_LEN], nonce: &[u8; NONCE_LEN]) -> ChaCha20Poly1305 {
    let mut key = Zeroizing::new([0; KEY_LEN]);
    Hkdf::<Sha256>::new(Some(nonce), file_key).expand(PAYLOAD_LABEL, key.as_mut()).expect("32 bytes fit HKDF-SHA256");
    ChaCha20Poly1305::new(Key::from_slice(key.as_ref()))
}

fn chunk_nonce(counter: u128, last: bool) -> Nonce {
    let mut nonce = Nonce::default();
    let counter = counter.to_be_bytes();
    nonce[..COUNTER_LEN].copy_from_slice(&counter[counter.len() - COUNTER_LEN..]);
    nonce[COUNTER_LEN] = if last { LAST_CHUNK } else { 0 };
    nonce
}

fn next_counter(counter: u128) -> Result<u128, PayloadError> {
    Some(counter + 1).filter(|next| *next < COUNTER_LIMIT).ok_or(PayloadError::TooManyChunks)
}

pub struct Encryptor<W: Write> {
    out: W,
    cipher: ChaCha20Poly1305,
    counter: u128,
    chunk: Zeroizing<Vec<u8>>,
}

impl<W: Write> Encryptor<W> {
    pub(super) fn new(out: W, file_key: &[u8; FILE_KEY_LEN], nonce: &[u8; NONCE_LEN]) -> Self {
        Self {
            out,
            cipher: payload_cipher(file_key, nonce),
            counter: 0,
            chunk: Zeroizing::new(Vec::with_capacity(SEALED_CHUNK_LEN)),
        }
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.seal_chunk(true)?;
        self.out.flush()?;
        Ok(self.out)
    }

    fn seal_chunk(&mut self, last: bool) -> io::Result<()> {
        self.cipher
            .encrypt_in_place(&chunk_nonce(self.counter, last), &[], &mut *self.chunk)
            .expect("the chunk has room for its tag");
        self.out.write_all(&self.chunk)?;
        self.chunk.clear();
        self.counter = next_counter(self.counter)?;
        Ok(())
    }
}

impl<W: Write> Write for Encryptor<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut rest = data;
        while !rest.is_empty() {
            if self.chunk.len() == CHUNK_LEN {
                self.seal_chunk(false)?;
            }
            let taken = rest.len().min(CHUNK_LEN - self.chunk.len());
            self.chunk.extend_from_slice(&rest[..taken]);
            rest = &rest[taken..];
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

#[derive(Clone, Copy)]
enum Failure {
    Payload(PayloadError),
    Armor(ArmorError),
    Io(io::ErrorKind),
}

impl Failure {
    fn of(error: &io::Error) -> Self {
        let inner = error.get_ref();
        if let Some(payload) = inner.and_then(|inner| inner.downcast_ref::<PayloadError>()) {
            return Self::Payload(*payload);
        }
        if let Some(armor) = inner.and_then(|inner| inner.downcast_ref::<ArmorError>()) {
            return Self::Armor(*armor);
        }
        Self::Io(error.kind())
    }
}

impl From<Failure> for io::Error {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Payload(error) => error.into(),
            Failure::Armor(error) => error.into(),
            Failure::Io(kind) => kind.into(),
        }
    }
}

#[derive(Clone, Copy)]
enum State {
    Open,
    Finished,
    Failed(Failure),
}

pub struct Decryptor<R: Read> {
    input: BufReader<Input<R>>,
    cipher: ChaCha20Poly1305,
    counter: u128,
    chunk: Zeroizing<Vec<u8>>,
    position: usize,
    state: State,
}

impl<R: Read> Decryptor<R> {
    pub(super) fn new(input: BufReader<Input<R>>, file_key: &[u8; FILE_KEY_LEN], nonce: &[u8; NONCE_LEN]) -> Self {
        Self {
            input,
            cipher: payload_cipher(file_key, nonce),
            counter: 0,
            chunk: Zeroizing::new(Vec::with_capacity(SEALED_CHUNK_LEN)),
            position: 0,
            state: State::Open,
        }
    }

    fn advance(&mut self) -> io::Result<()> {
        self.position = 0;
        self.chunk.resize(SEALED_CHUNK_LEN, 0);
        let opened = self.open_next();
        if let Err(error) = &opened {
            self.chunk.clear();
            self.state = State::Failed(Failure::of(error));
        }
        opened
    }

    fn open_next(&mut self) -> io::Result<()> {
        let read = read_full(&mut self.input, &mut self.chunk)?;
        self.chunk.truncate(read);
        if read == 0 {
            return Err(PayloadError::Truncated.into());
        }
        let short = read < SEALED_CHUNK_LEN;
        if short && read == TAG_LEN && self.counter > 0 {
            return Err(PayloadError::EmptyLastChunk.into());
        }
        let last = short || self.open_in_place(false).is_err();
        if last {
            self.open_in_place(true)?;
        }
        self.counter = next_counter(self.counter)?;
        if last {
            self.state = match at_end(&mut self.input) {
                Ok(true) => State::Finished,
                Ok(false) => State::Failed(Failure::Payload(PayloadError::TrailingData)),
                Err(error) => State::Failed(Failure::of(&error)),
            };
        }
        Ok(())
    }

    fn open_in_place(&mut self, last: bool) -> Result<(), PayloadError> {
        self.cipher
            .decrypt_in_place(&chunk_nonce(self.counter, last), &[], &mut *self.chunk)
            .map_err(|_| PayloadError::Chunk)
    }
}

fn at_end(input: &mut impl BufRead) -> io::Result<bool> {
    loop {
        match input.fill_buf() {
            Ok(rest) => return Ok(rest.is_empty()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

impl<R: Read> Read for Decryptor<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        while self.position == self.chunk.len() {
            match self.state {
                State::Open => self.advance()?,
                State::Finished => return Ok(0),
                State::Failed(failure) => return Err(failure.into()),
            }
        }
        let copied = out.len().min(self.chunk.len() - self.position);
        out[..copied].copy_from_slice(&self.chunk[self.position..self.position + copied]);
        self.position += copied;
        Ok(copied)
    }
}
