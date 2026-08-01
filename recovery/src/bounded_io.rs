// SPDX-License-Identifier: BSD-3-Clause
// Resource limits shared by Recovery HTTP clients and request handlers.

use std::io::{self, Read, Write};

pub struct BodyLimit {
    declared_length: Option<usize>,
    max_size: usize,
    received: usize,
}

impl BodyLimit {
    pub fn new(declared_length: Option<usize>, max_size: usize) -> io::Result<Self> {
        if declared_length.is_some_and(|length| length > max_size) {
            return Err(limit_error(max_size));
        }
        Ok(Self {
            declared_length,
            max_size,
            received: 0,
        })
    }

    pub fn observe(&mut self, count: usize) -> io::Result<()> {
        self.received = self
            .received
            .checked_add(count)
            .ok_or_else(|| limit_error(self.max_size))?;
        if self.received > self.max_size
            || self
                .declared_length
                .is_some_and(|length| self.received > length)
        {
            return Err(limit_error(self.max_size));
        }
        Ok(())
    }

    pub fn finish(self) -> io::Result<usize> {
        if self
            .declared_length
            .is_some_and(|length| self.received != length)
        {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "body length does not match Content-Length",
            ));
        }
        Ok(self.received)
    }
}

pub fn validate_framing(
    content_length: Option<&str>,
    transfer_encoding: Option<&str>,
    max_size: usize,
) -> io::Result<Option<usize>> {
    if transfer_encoding.is_some_and(|value| !value.trim().is_empty()) {
        return Err(invalid_data("transfer-encoded bodies are not accepted"));
    }

    let Some(content_length) = content_length else {
        return Ok(None);
    };
    let declared = content_length
        .trim()
        .parse::<usize>()
        .map_err(|_| invalid_data("invalid Content-Length"))?;
    if declared > max_size {
        return Err(limit_error(max_size));
    }
    Ok(Some(declared))
}

pub fn read_to_vec_limited<R: Read>(
    reader: R,
    declared_length: Option<usize>,
    max_size: usize,
) -> io::Result<Vec<u8>> {
    let mut output = Vec::with_capacity(declared_length.unwrap_or(0).min(max_size));
    copy_limited(reader, &mut output, declared_length, max_size)?;
    Ok(output)
}

pub fn copy_limited<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    declared_length: Option<usize>,
    max_size: usize,
) -> io::Result<usize> {
    let mut limit = BodyLimit::new(declared_length, max_size)?;
    let mut buffer = [0u8; 4096];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        limit.observe(count)?;
        writer.write_all(&buffer[..count])?;
    }
    limit.finish()
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn limit_error(max_size: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("body exceeds {max_size}-byte limit"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn exact_known_length_succeeds() {
        let declared = validate_framing(Some("8"), None, 8).unwrap();
        let body = read_to_vec_limited(Cursor::new(b"12345678"), declared, 8).unwrap();
        assert_eq!(body, b"12345678");
    }

    #[test]
    fn one_byte_over_known_length_fails_before_reading() {
        let error = validate_framing(Some("9"), None, 8).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn chunked_and_other_transfer_encodings_are_rejected() {
        assert!(validate_framing(None, Some("chunked"), 8).is_err());
        assert!(validate_framing(None, Some("gzip, chunked"), 8).is_err());
    }

    #[test]
    fn unknown_length_is_enforced_during_streaming() {
        assert!(read_to_vec_limited(Cursor::new(b"12345678"), None, 8).is_ok());
        assert!(read_to_vec_limited(Cursor::new(b"123456789"), None, 8).is_err());
    }

    #[test]
    fn response_longer_or_shorter_than_declared_is_rejected() {
        assert!(read_to_vec_limited(Cursor::new(b"12345"), Some(4), 8).is_err());
        let error = read_to_vec_limited(Cursor::new(b"123"), Some(4), 8).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn oversized_post_body_is_rejected_without_content_length() {
        let request = vec![b'x'; 1025];
        assert!(read_to_vec_limited(Cursor::new(request), None, 1024).is_err());
    }
}
