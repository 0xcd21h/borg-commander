use std::io::{self, BufRead, BufReader, Read};

use base64::alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::{DecodePaddingMode, Engine as _};

const BEGIN: &[u8] = b"-----BEGIN AGE ENCRYPTED FILE-----";
const END: &[u8] = b"-----END AGE ENCRYPTED FILE-----";
const COLUMNS_PER_LINE: usize = 64;
const BYTES_PER_LINE: usize = COLUMNS_PER_LINE / 4 * 3;
const WHITESPACE_LIMIT: usize = 1024;
const LINE_LIMIT: usize = WHITESPACE_LIMIT;
const LINE_END: u8 = b'\n';
const CARRIAGE_RETURN: u8 = b'\r';

const PADDED_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::RequireCanonical)
        .with_decode_allow_trailing_bits(false),
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ArmorError {
    #[error("the first line is not the age armor line")]
    FirstLine,
    #[error("the armor ends early")]
    Truncated,
    #[error("an armor line is empty")]
    EmptyLine,
    #[error("an armor line is too long")]
    LongLine,
    #[error("an armor line is not valid base64")]
    Base64,
    #[error("the armor does not end with its closing line")]
    ClosingLine,
    #[error("there is data after the armor")]
    TrailingData,
    #[error("there is too much whitespace around the armor")]
    Whitespace,
}

impl From<ArmorError> for io::Error {
    fn from(error: ArmorError) -> Self {
        io::Error::new(io::ErrorKind::InvalidData, error)
    }
}

enum Fault {
    Armor(ArmorError),
    Io(io::Error),
}

impl From<ArmorError> for Fault {
    fn from(error: ArmorError) -> Self {
        Self::Armor(error)
    }
}

impl From<io::Error> for Fault {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Copy)]
enum End {
    Clean,
    Armor(ArmorError),
    Io(io::ErrorKind),
}

impl Fault {
    fn end(&self) -> End {
        match self {
            Self::Armor(error) => End::Armor(*error),
            Self::Io(error) => End::Io(error.kind()),
        }
    }

    fn into_io(self) -> io::Error {
        match self {
            Self::Armor(error) => error.into(),
            Self::Io(error) => error,
        }
    }
}

pub(super) struct ArmorReader<R: Read> {
    input: BufReader<R>,
    started: bool,
    decoded: [u8; BYTES_PER_LINE],
    filled: usize,
    position: usize,
    end: Option<End>,
}

impl<R: Read> ArmorReader<R> {
    pub fn new(input: R) -> Self {
        Self {
            input: BufReader::new(input),
            started: false,
            decoded: [0; BYTES_PER_LINE],
            filled: 0,
            position: 0,
            end: None,
        }
    }

    fn step(&mut self) -> Result<(), Fault> {
        if !self.started {
            self.start()?;
        }
        let line = self.line()?.0;
        if line == END {
            self.drain()?;
            self.end = Some(End::Clean);
            return Ok(());
        }
        if line.is_empty() {
            return Err(ArmorError::EmptyLine.into());
        }
        if line.len() > COLUMNS_PER_LINE {
            return Err(ArmorError::LongLine.into());
        }
        let filled = PADDED_BASE64.decode_slice(&line, &mut self.decoded).map_err(|_| ArmorError::Base64)?;
        if filled < BYTES_PER_LINE {
            if self.line()?.0 != END {
                return Err(ArmorError::ClosingLine.into());
            }
            self.end = Some(self.drain().map_or_else(|fault| fault.end(), |()| End::Clean));
        }
        self.filled = filled;
        self.position = 0;
        Ok(())
    }

    fn start(&mut self) -> Result<(), Fault> {
        let mut skipped = 0;
        loop {
            let (line, read) = self.line()?;
            if line != BEGIN && line.iter().all(|byte| is_space(*byte)) {
                skipped += read;
                if skipped > WHITESPACE_LIMIT {
                    return Err(ArmorError::Whitespace.into());
                }
                continue;
            }
            if line != BEGIN {
                return Err(ArmorError::FirstLine.into());
            }
            self.started = true;
            return Ok(());
        }
    }

    fn line(&mut self) -> Result<(Vec<u8>, usize), Fault> {
        let mut line = Vec::new();
        let read = (&mut self.input).take(LINE_LIMIT as u64).read_until(LINE_END, &mut line)?;
        if read == 0 {
            return Err(ArmorError::Truncated.into());
        }
        if line.last() == Some(&LINE_END) {
            line.pop();
        } else if read == LINE_LIMIT {
            return Err(ArmorError::LongLine.into());
        }
        if line.last() == Some(&CARRIAGE_RETURN) {
            line.pop();
        }
        Ok((line, read))
    }

    fn drain(&mut self) -> Result<(), Fault> {
        let mut rest = Vec::new();
        (&mut self.input).take(WHITESPACE_LIMIT as u64).read_to_end(&mut rest)?;
        if !rest.iter().all(|byte| is_space(*byte)) {
            return Err(ArmorError::TrailingData.into());
        }
        if rest.len() == WHITESPACE_LIMIT {
            return Err(ArmorError::Whitespace.into());
        }
        Ok(())
    }
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c')
}

impl<R: Read> Read for ArmorReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        while self.position == self.filled {
            match self.end {
                Some(End::Clean) => return Ok(0),
                Some(End::Armor(error)) => return Err(error.into()),
                Some(End::Io(kind)) => return Err(kind.into()),
                None => {
                    if let Err(fault) = self.step() {
                        self.filled = 0;
                        self.position = 0;
                        self.end = Some(fault.end());
                        return Err(fault.into_io());
                    }
                }
            }
        }
        let copied = out.len().min(self.filled - self.position);
        out[..copied].copy_from_slice(&self.decoded[self.position..self.position + copied]);
        self.position += copied;
        Ok(copied)
    }
}
