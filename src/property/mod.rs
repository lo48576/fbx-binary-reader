//! Contains node property related stuff.

use std::fmt;
use byteorder::{LittleEndian, ReadBytesExt};
use error::{Error, Result};
use reader::ParserConfig;

pub struct DelayedPropertiesIter {
    buffer: Vec<u8>,
    current_offset: usize,
    num_properties: usize,
    failed: bool,
}

impl DelayedPropertiesIter {
    pub fn from_vec_u8(vec: Vec<u8>, _version: i32, _config: &ParserConfig, num_properties: usize) -> Self {
        DelayedPropertiesIter {
            buffer: vec,
            current_offset: 0,
            num_properties: num_properties,
            failed: false,
        }
    }

    pub fn is_err(&self) -> bool {
        self.failed
    }

    pub fn iter(&mut self) -> &mut Self {
        self
    }
}

impl fmt::Debug for DelayedPropertiesIter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("DelayedPropertiesIter")
            .field("current_offset", &self.current_offset)
            .field("num_properties", &self.num_properties)
            .field("failed", &self.failed)
            .finish()
    }
}

impl<'a> Iterator for &'a DelayedPropertiesIter {
    type Item = Property<'a>;

    fn next(&mut self) -> Option<Property<'a>> {
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.num_properties))
    }
}

pub enum Property<'a> {
    Dummy(&'a [u8]),
}
