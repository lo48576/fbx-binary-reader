//! Contains node property related stuff.

use std::fmt;
use std::str;
use std::io::Read;
use byteorder::{LittleEndian, ReadBytesExt};
use flate2::read::ZlibDecoder;


macro_rules! try_opt {
    ($opt:expr) => (if let Some(val) = $opt {
        val
    } else {
        return None;
    });
}


#[derive(Clone)]
pub struct DelayedProperties {
    pub buffer: Vec<u8>,
    pub num_properties: usize,
}

impl DelayedProperties {
    pub fn from_vec_u8(vec: Vec<u8>, _version: i32, num_properties: usize) -> Self {
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

    pub fn num_properties(&self) -> usize {
        self.num_properties
    }
}

impl fmt::Debug for DelayedProperties {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("DelayedProperties")
            .field("buffer_size", &self.buffer.len())
            .field("num_properties", &self.num_properties)
            .finish()
    }
}

pub struct Iter<'a> {
    buffer: &'a [u8],
    rest_properties: usize,
}

impl<'a> Iter<'a> {
    fn read_u8(&mut self) -> Option<u8> {
        const SIZE: usize = 1;
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_u8().unwrap();
        Some(val)
    }

    fn read_u32(&mut self) -> Option<u32> {
        const SIZE: usize = 4;
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_u32::<LittleEndian>().unwrap();
        Some(val)
    }

    fn read_i16(&mut self) -> Option<i16> {
        const SIZE: usize = 2;
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_i16::<LittleEndian>().unwrap();
        Some(val)
    }

    fn read_i32(&mut self) -> Option<i32> {
        const SIZE: usize = 4;
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_i32::<LittleEndian>().unwrap();
        Some(val)
    }

    fn read_i64(&mut self) -> Option<i64> {
        const SIZE: usize = 8;
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_i64::<LittleEndian>().unwrap();
        Some(val)
    }

    fn read_f32(&mut self) -> Option<f32> {
        const SIZE: usize = 4;
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_f32::<LittleEndian>().unwrap();
        Some(val)
    }

    fn read_f64(&mut self) -> Option<f64> {
        const SIZE: usize = 8;
        if self.buffer.len() < SIZE {
            error!("Property data is too short");
            self.rest_properties = 0;
            return None;
        }
        let val = self.buffer.read_f64::<LittleEndian>().unwrap();
        Some(val)
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
                let strbuf = try_opt!(str::from_utf8(buf).map_err(|err| {
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
                let array_header = if let Some((header, length)) = ArrayHeader::from_binary(self.buffer) {
                    self.buffer = &self.buffer[length..];
                    header
                } else {
                    error!("Property data is too short");
                    self.rest_properties = 0;
                    return None;
                };
                let buffer = if self.buffer.len() < array_header.compressed_length {
                    error!("Property data is too short");
                    self.rest_properties = 0;
                    return None;
                } else {
                    let bufs = self.buffer.split_at(array_header.compressed_length);
                    self.buffer = bufs.1;
                    bufs.0
                };
                if let Some(val) = read_property_array(buffer, &array_header, type_code) {
                    self.rest_properties -= 1;
                    Some(val)
                } else {
                    self.rest_properties = 0;
                    None
                }
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

/// Header of array type property value.
struct ArrayHeader {
    /// Number of values in the array, *NOT byte size*.
    num_elements: usize,
    /// Denotes whether data in stream is plain, or what algorithm it is compressed by.
    encoding: u32,
    /// Byte size of the compressed array value in the stream.
    compressed_length: usize,
}

impl ArrayHeader {
    /// Constructs `ArrayValueHeader` from the given binary.
    pub fn from_binary(source: &[u8]) -> Option<(Self, usize)> {
        const LENGTH: usize = 4 * 3;
        let mut buffer = source;
        if buffer.len() < LENGTH {
            return None;
        }
        // `buffer` has enough length of data. `read_u32()`s must success.
        let num_elements = buffer.read_u32::<LittleEndian>().unwrap() as usize;
        let encoding = buffer.read_u32::<LittleEndian>().unwrap();
        let compressed_length = buffer.read_u32::<LittleEndian>().unwrap() as usize;
        Some((ArrayHeader {
            num_elements: num_elements,
            encoding: encoding,
            compressed_length: compressed_length,
        }, LENGTH))
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
    /// Array of 4-byte signed integer.
    VecI32(Vec<i32>),
    /// Array of 8-byte signed integer.
    VecI64(Vec<i64>),
    /// Array of 4-byte single-precision IEEE 754 number.
    VecF32(Vec<f32>),
    /// Array of 8-byte double-precision IEEE 754 number.
    VecF64(Vec<f64>),
}

fn read_property_array<'a>(mut buffer: &'a [u8], header: &ArrayHeader, type_code: u8) -> Option<Property<'static>> {
    match header.encoding {
        // 0: raw.
        0 => read_property_array_from_plain_stream(&mut buffer, header, type_code),
        // 1: zlib compressed.
        1 => read_property_array_from_plain_stream(&mut ZlibDecoder::new(buffer), header, type_code),
        // Unknown.
        e => {
            error!("Unknown property array encoding: encoding={}", e);
            None
        },
    }
}

fn read_property_array_from_plain_stream<R: Read>(reader: &mut R, header: &ArrayHeader, type_code: u8) -> Option<Property<'static>> {
    macro_rules! read_into_vec {
        ($t:ty, $read_fun:ident, $variant:ident) => ({
            let mut data = Vec::<$t>::with_capacity(header.num_elements);
            for _ in 0..header.num_elements {
                data.push(try_opt!(reader.$read_fun::<LittleEndian>().ok()));
            }
            Property::$variant(data)
        });
    }
    Some(match type_code {
        // Array of 4-byte signed integer.
        b'b' => {
            let mut data = Vec::<bool>::with_capacity(header.num_elements);
            // Don't check whether the values are 'T's and 'Y's.
            for _ in 0..header.num_elements {
                data.push(try_opt!(reader.read_u8().ok()) & 1 == 1);
            }
            Property::VecBool(data)
        },
        // Array of 4-byte signed integer.
        b'i' => read_into_vec!(i32, read_i32, VecI32),
        // Array of 8-byte signed integer.
        b'l' => read_into_vec!(i64, read_i64, VecI64),
        // Array of 4-byte single-precision IEEE 754 floating-point number.
        b'f' => read_into_vec!(f32, read_f32, VecF32),
        // Array of 8-byte single-precision IEEE 754 floating-point number.
        b'd' => read_into_vec!(f64, read_f64, VecF64),
        _ => unreachable!(),
    })
}
