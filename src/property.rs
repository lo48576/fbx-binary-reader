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

    pub fn iter(&self) -> PropertiesIter {
        PropertiesIter {
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

pub struct PropertiesIter<'a> {
    buffer: &'a [u8],
    rest_properties: usize,
}

macro_rules! implement_iter_read {
    ($t:ty, $read_fun:ident, $size:expr) => (
        impl<'a> PropertiesIter<'a> {
            fn $read_fun(&mut self) -> Option<$t> {
                // TODO: Get size from `$t` at compile time.
                //const SIZE: usize = ::std::mem::size_of::<$t>(); // size_of() is not `const fn`.
                const SIZE: usize = $size;
                if self.buffer.len() < SIZE {
                    error!("Property data is too short");
                    self.rest_properties = 0;
                    return None;
                }
                let val = self.buffer.$read_fun::<LittleEndian>().unwrap();
                Some(val)
            }
        }
    )
}

impl<'a> PropertiesIter<'a> {
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
}
implement_iter_read!(u32, read_u32, 4);
implement_iter_read!(i16, read_i16, 2);
implement_iter_read!(i32, read_i32, 4);
implement_iter_read!(i64, read_i64, 8);
implement_iter_read!(f32, read_f32, 4);
implement_iter_read!(f64, read_f64, 8);

impl<'a> Iterator for PropertiesIter<'a> {
    type Item = Property<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        macro_rules! read_primitive_prop {
            ($read_fun:ident, $variant:ident) => ({
                let val = try_opt!(self.$read_fun());
                self.rest_properties -= 1;
                Some(Property::$variant(val))
            })
        }
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
            b'Y' => read_primitive_prop!(read_i16, I16),
            // 4-byte signed integer.
            b'I' => read_primitive_prop!(read_i32, I32),
            // 8-byte signed integer.
            b'L' => read_primitive_prop!(read_i64, I64),
            // 4-byte single-precision IEEE 754 floating-point number.
            b'F' => read_primitive_prop!(read_f32, F32),
            // 8-byte single-precision IEEE 754 floating-point number.
            b'D' => read_primitive_prop!(read_f64, F64),
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
                let str_or_raw = str::from_utf8(buf).map_err(|err| {
                    warn!("Property value of string type is invalid as UTF-8 sequence: {}", err);
                    buf
                });
                Some(Property::String(str_or_raw))
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
                if self.buffer.len() < array_header.compressed_length {
                    error!("Property data is too short");
                    self.rest_properties = 0;
                    return None;
                }
                let buf = &self.buffer[0..array_header.compressed_length];
                self.buffer = &self.buffer[array_header.compressed_length..];
                if let Some(val) = read_property_array(buf, &array_header, type_code) {
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
    String(Result<&'a str, &'a [u8]>),
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

macro_rules! implement_property_value_getter {
    (primitive, $t:ty, $method_name:ident, $variant:ident) => (
        impl<'a> Property<'a> {
            /// Get property value without consuming self.
            ///
            /// Tries to get property value of specific type without type conversion.
            pub fn $method_name(&self) -> Option<$t> {
                match *self {
                    Property::$variant(v) => Some(v),
                    _ => None,
                }
            }
        }
    );
    (vec, $t:ty, $method_name:ident, $variant:ident) => (
        impl<'a> Property<'a> {
            /// Get property value without consuming self.
            ///
            /// Tries to get property value of specific type without type conversion.
            pub fn $method_name(&self) -> Option<&Vec<$t>> {
                match *self {
                    Property::$variant(ref v) => Some(v),
                    _ => None,
                }
            }
        }
    );
}

implement_property_value_getter!(primitive, bool, get_bool, Bool);
implement_property_value_getter!(primitive, i16, get_i16, I16);
implement_property_value_getter!(primitive, i32, get_i32, I32);
implement_property_value_getter!(primitive, i64, get_i64, I64);
implement_property_value_getter!(primitive, f32, get_f32, F32);
implement_property_value_getter!(primitive, f64, get_f64, F64);

implement_property_value_getter!(vec, bool, get_vec_bool, VecBool);
implement_property_value_getter!(vec, i32, get_vec_i32, VecI32);
implement_property_value_getter!(vec, i64, get_vec_i64, VecI64);
implement_property_value_getter!(vec, f32, get_vec_f32, VecF32);
implement_property_value_getter!(vec, f64, get_vec_f64, VecF64);

implement_property_value_getter!(primitive, &'a [u8], get_binary, Binary);
implement_property_value_getter!(primitive, Result<&'a str, &'a [u8]>, get_string_or_raw, String);

impl<'a> Property<'a> {
    /// Get property value without consuming self.
    ///
    /// Tries to get property value of specific type without type conversion.
    pub fn get_string(&self) -> Option<&'a str> {
        match *self {
            Property::String(Ok(ref v)) => Some(v),
            _ => None,
        }
    }
}

macro_rules! implement_property_value_into {
    ($t:ty, $method_name:ident, $variant:ident) => (
        impl<'a> Property<'a> {
            /// Get property value consuming self.
            ///
            /// Tries to get property value of specific type without type conversion.
            pub fn $method_name(self) -> Result<$t, Self> {
                match self {
                    Property::$variant(v) => Ok(v),
                    s => Err(s),
                }
            }
        }
    );
}

implement_property_value_into!(bool, into_bool, Bool);
implement_property_value_into!(i16, into_i16, I16);
implement_property_value_into!(i32, into_i32, I32);
implement_property_value_into!(i64, into_i64, I64);
implement_property_value_into!(f32, into_f32, F32);
implement_property_value_into!(f64, into_f64, F64);
implement_property_value_into!(Vec<bool>, into_vec_bool, VecBool);
implement_property_value_into!(Vec<i32>, into_vec_i32, VecI32);
implement_property_value_into!(Vec<i64>, into_vec_i64, VecI64);
implement_property_value_into!(Vec<f32>, into_vec_f32, VecF32);
implement_property_value_into!(Vec<f64>, into_vec_f64, VecF64);

impl<'a> Property<'a> {
    /// Safe conversion.
    ///
    /// Tries to convert property value into specific type without data loss.
    pub fn as_i32(&self) -> Option<i32> {
        match *self {
            Property::I16(v) => Some(v as i32),
            Property::I32(v) => Some(v),
            _ => None,
        }
    }

    /// Safe conversion.
    ///
    /// Tries to convert property value into specific type without data loss.
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Property::I16(v) => Some(v as i64),
            Property::I32(v) => Some(v as i64),
            Property::I64(v) => Some(v),
            _ => None,
        }
    }

    /// Safe conversion.
    ///
    /// Tries to convert property value into specific type.
    pub fn as_f32(&self) -> Option<f32> {
        match *self {
            Property::F32(v) => Some(v),
            Property::F64(v) => Some(v as f32),
            _ => None,
        }
    }

    /// Safe conversion.
    ///
    /// Tries to convert property value into specific type.
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Property::F32(v) => Some(v as f64),
            Property::F64(v) => Some(v),
            _ => None,
        }
    }

    /// Safe conversion consuming self.
    ///
    /// Tries to convert property value into specific type without data loss.
    pub fn as_vec_i64(self) -> Option<Vec<i64>> {
        match self {
            Property::VecI32(v) => Some(v.into_iter().map(|v| v as i64).collect::<Vec<_>>()),
            Property::VecI64(v) => Some(v),
            _ => None,
        }
    }

    /// Safe conversion consuming self.
    ///
    /// Tries to convert property value into specific type.
    pub fn as_vec_f32(self) -> Option<Vec<f32>> {
        match self {
            Property::VecF32(v) => Some(v),
            Property::VecF64(v) => Some(v.into_iter().map(|v| v as f32).collect::<Vec<_>>()),
            _ => None,
        }
    }

    /// Safe conversion consuming self.
    ///
    /// Tries to convert property value into specific type.
    pub fn as_vec_f64(self) -> Option<Vec<f64>> {
        match self {
            Property::VecF32(v) => Some(v.into_iter().map(|v| v as f64).collect::<Vec<_>>()),
            Property::VecF64(v) => Some(v),
            _ => None,
        }
    }
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
