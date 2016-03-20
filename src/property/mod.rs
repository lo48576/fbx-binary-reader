//! Contains node property related stuff.

use std::fmt;
use byteorder::{LittleEndian, ReadBytesExt};
use error::{Error, Result};
use reader::ParserConfig;


macro_rules! try_opt {
    ($opt:expr) => (if let Some(val) = $opt {
        val
    } else {
        return None;
    });
}


pub struct DelayedProperties {
    pub buffer: Vec<u8>,
    pub num_properties: usize,
}

impl DelayedProperties {
    pub fn from_vec_u8(vec: Vec<u8>, _version: i32, _config: &ParserConfig, num_properties: usize) -> Self {
        DelayedProperties {
            buffer: vec,
            num_properties: num_properties,
        }
    }

    pub fn iter(&self) -> Iter {
        Iter {
            buffer: &self.buffer[..],
            rest_properties: self.num_properties,
        }
    }
}

impl fmt::Debug for DelayedProperties {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("DelayedProperties")
            .field("num_properties", &self.num_properties)
            .finish()
    }
}

pub struct Iter<'a> {
    buffer: &'a [u8],
    rest_properties: usize,
}

impl<'a> Iter<'a> {
    pub fn read_u8(&mut self) -> Option<u8> {
        const SIZE: usize = 1;
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_u8().unwrap();
        Some(val)
    }

    pub fn read_u32(&mut self) -> Option<u32> {
        const SIZE: usize = 4;
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_u32::<LittleEndian>().unwrap();
        Some(val)
    }

    pub fn read_i16(&mut self) -> Option<i16> {
        const SIZE: usize = 2;
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_i16::<LittleEndian>().unwrap();
        Some(val)
    }

    pub fn read_i32(&mut self) -> Option<i32> {
        const SIZE: usize = 4;
println!("buffer_len={}", self.buffer.len());
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_i32::<LittleEndian>().unwrap();
        Some(val)
    }

    pub fn read_i64(&mut self) -> Option<i64> {
        const SIZE: usize = 8;
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_i64::<LittleEndian>().unwrap();
        Some(val)
    }

    pub fn read_f32(&mut self) -> Option<f32> {
        const SIZE: usize = 4;
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_f32::<LittleEndian>().unwrap();
        Some(val)
    }

    pub fn read_f64(&mut self) -> Option<f64> {
        const SIZE: usize = 8;
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_f64::<LittleEndian>().unwrap();
        Some(val)
    }

    pub fn read_exact(&mut self, size: usize) -> Option<&[u8]> {
        if self.buffer.len() < size {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let ret_buf = &self.buffer[0..size];
        self.buffer = &self.buffer[size..];
        Some(ret_buf)
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = Property<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest_properties == 0 {
            return None;
        }
        let type_code = try_opt!(self.read_u8());
        match type_code {
            // Boolean.
            b'C' => {
                let val = try_opt!(self.read_u8());
                if (val != b'T') && (val != b'Y') {
                    warn!("Expected 0x54 ('T') or 0x59 ('Y') as boolean property value, but got {:#x}", val);
                }
                self.rest_properties -= 1;
                Some(Property::Bool(val & 1 == 1))
            },
            // 2-byte signed integer.
            b'Y' => {
                let val = try_opt!(self.read_i16());
                self.rest_properties -= 1;
                Some(Property::I16(val))
            },
            // 4-byte signed integer.
            b'I' => {
                let val = try_opt!(self.read_i32());
                self.rest_properties -= 1;
                Some(Property::I32(val))
            },
            // 8-byte signed integer.
            b'L' => {
                let val = try_opt!(self.read_i64());
                self.rest_properties -= 1;
                Some(Property::I64(val))
            },
            // 4-byte single-precision IEEE 754 floating-point number.
            b'F' => {
                let val = try_opt!(self.read_f32());
                self.rest_properties -= 1;
                Some(Property::F32(val))
            },
            // 8-byte single-precision IEEE 754 floating-point number.
            b'D' => {
                let val = try_opt!(self.read_f64());
                self.rest_properties -= 1;
                Some(Property::F64(val))
            },
            // String.
            b'S' => {
                let length = try_opt!(self.read_u32()) as usize;
                if self.buffer.len() < length {
                    error!("Property data is too short");
                    self.rest_properties = 0;
                    return None;
                }
                let buf = &self.buffer[0..length];
                self.buffer = &self.buffer[length..];
                self.rest_properties -= 1;
                let strbuf = try_opt!(::std::str::from_utf8(buf).map_err(|err| {
                    error!("Failed to decode a property of string type: {}", err);
                    self.rest_properties = 0;
                }).ok());
                Some(Property::String(strbuf))
            },
            // Raw binary.
            b'R' => {
                let length = try_opt!(self.read_u32()) as usize;
                if self.buffer.len() < length {
                    error!("Property data is too short");
                    self.rest_properties = 0;
                    return None;
                }
                let buf = &self.buffer[0..length];
                self.buffer = &self.buffer[length..];
                self.rest_properties -= 1;
                Some(Property::Binary(buf))
            },
            b'b' | b'i' | b'l' | b'f' | b'd' => {
                warn!("Not yet supported: type_code='{}'", type_code);
                self.rest_properties = 0;
                None
            },
            _ => {
                error!("Unknown type code: {:#x}", type_code);
                self.rest_properties = 0;
                None
            },
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.rest_properties))
    }
}

#[derive(Debug)]
pub enum Property<'a> {
    /// Boolean.
    Bool(bool),
    /// 2-byte signed integer.
    I16(i16),
    /// 4-byte signed integer.
    I32(i32),
    /// 8-byte signed integer.
    I64(i64),
    /// 4-byte single-precision IEEE 754 floating-point number.
    F32(f32),
    /// 8-byte single-precision IEEE 754 floating-point number.
    F64(f64),
    /// String.
    String(&'a str),
    /// Raw binary.
    Binary(&'a [u8]),
    /// Array of boolean.
    VecBool(Vec<bool>),
}
