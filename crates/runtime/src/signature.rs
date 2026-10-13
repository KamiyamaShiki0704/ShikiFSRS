//! Byte signatures must identify one position. Wildcards alone are rejected.
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    bytes: Vec<Option<u8>>,
    anchor: usize,
}

impl std::str::FromStr for Pattern {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        let mut bytes = Vec::new();
        for token in text.split_whitespace() {
            let value = match token {
                "?" | "??" => None,
                token if token.len() == 2 && token.bytes().all(|b| b.is_ascii_hexdigit()) => Some(
                    u8::from_str_radix(token, 16).map_err(|_| Error::InvalidPattern {
                        token: token.to_owned(),
                    })?,
                ),
                _ => {
                    return Err(Error::InvalidPattern {
                        token: token.to_owned(),
                    });
                }
            };
            bytes.push(value);
        }
        let anchor = bytes
            .iter()
            .position(Option::is_some)
            .ok_or(Error::EmptyPattern)?;
        Ok(Self { bytes, anchor })
    }
}

impl Pattern {
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn find_unique(&self, input: &[u8]) -> Result<usize> {
        if input.len() < self.len() {
            return Err(Error::PatternNotFound);
        }
        let mut result = None;
        for start in 0..=input.len() - self.len() {
            if Some(input[start + self.anchor]) != self.bytes[self.anchor] {
                continue;
            }
            if self
                .bytes
                .iter()
                .enumerate()
                .all(|(i, expected)| expected.is_none_or(|value| input[start + i] == value))
            {
                if let Some(first) = result {
                    return Err(Error::AmbiguousPattern {
                        first,
                        second: start,
                    });
                }
                result = Some(start);
            }
        }
        result.ok_or(Error::PatternNotFound)
    }
}
