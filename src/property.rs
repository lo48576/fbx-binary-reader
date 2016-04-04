//! Contains node property related stuff.

use std::borrow::Cow;
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
    buffer: Vec<u8>,
    num_properties: usize,
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
    fn from_binary(source: &[u8]) -> Option<(Self, usize)> {
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

/// Node property.
///
/// # Getters
///
/// * `get_*` doesn't convert types and doesn't consume `self`.
/// * `as_*` converts types safely but doesn't consume `self`.
/// * `extract_*` doesn't convert types safely and consumes `self`.
/// * `into_*` converts types safely and consumes `self`.
///
/// | Prefix     | convert types | consume self |
/// |:-----------|--------------:|-------------:|
/// | `get_`     | no            | no           |
/// | `as_`      | yes           | no           |
/// | `extract_` | no            | yes          |
/// | `into_`    | yes           | yes          |
///
/// - `get_foo` and `as_foo` returns `Option<Foo>`.
/// - `extract_foo` and `into_foo` returns `Result<Foo, Property>`.
///
/// - `get_*` is available for all types.
/// - `extract_*` is available for all types *except `string`, `string_or_raw` and `binary`*.
/// - `into_*` and `as_*` is available only for types which is safely converted to.
///   * `i16` -> `i32`, `i16` -> `i64`, and `i32` -> `i64` are considered "safe".
///   * `f32` -> `f64`, `f64` -> `f32` are considered "safe".
///   * If a conversion `T` -> `U` is "safe", `Vec<T>` -> `Vec<U>` is also "safe".
///
/// Getter return types:
///
/// | Method suffix   | Wrapped result type   |
/// |:----------------|:----------------------|
/// | `bool`          | `bool`                |
/// | `i16`           | `i16`                 |
/// | `i32`           | `i32`                 |
/// | `i64`           | `i64`                 |
/// | `f32`           | `f32`                 |
/// | `f64`           | `f64`                 |
/// | `string_or_raw` | `Result<&str, &[u8]>` |
/// | `string`        | `&str`                |
/// | `binary`        | `&[u8]`               |
/// | `vec_bool`      | `Vec<bool>`           |
/// | `vec_i32`       | `Vec<i32>`            |
/// | `vec_i64`       | `Vec<i64>`            |
/// | `vec_f32`       | `Vec<f32>`            |
/// | `vec_f64`       | `Vec<f64>`            |
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

// Not convert type, not consume self.
macro_rules! implement_getter_get {
    (primitive, $t:ty, $method_name:ident, $variant:ident) => (
        impl<'a> Property<'a> {
            /// Get property value without consuming self, without type conversion.
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
            /// Get property value without consuming self, without type conversion.
            pub fn $method_name(&self) -> Option<&Vec<$t>> {
                match *self {
                    Property::$variant(ref v) => Some(v),
                    _ => None,
                }
            }
        }
    );
}

implement_getter_get!(primitive, bool, get_bool, Bool);
implement_getter_get!(primitive, i16, get_i16, I16);
implement_getter_get!(primitive, i32, get_i32, I32);
implement_getter_get!(primitive, i64, get_i64, I64);
implement_getter_get!(primitive, f32, get_f32, F32);
implement_getter_get!(primitive, f64, get_f64, F64);

implement_getter_get!(vec, bool, get_vec_bool, VecBool);
implement_getter_get!(vec, i32, get_vec_i32, VecI32);
implement_getter_get!(vec, i64, get_vec_i64, VecI64);
implement_getter_get!(vec, f32, get_vec_f32, VecF32);
implement_getter_get!(vec, f64, get_vec_f64, VecF64);

implement_getter_get!(primitive, &'a [u8], get_binary, Binary);
implement_getter_get!(primitive, Result<&'a str, &'a [u8]>, get_string_or_raw, String);

impl<'a> Property<'a> {
    /// Get property value without consuming self, without type conversion.
    pub fn get_string(&self) -> Option<&'a str> {
        match *self {
            Property::String(Ok(ref v)) => Some(v),
            _ => None,
        }
    }
}


// Not convert type, consume self.
macro_rules! implement_getter_extract {
    (primitive, $t:ty, $method_name:ident, $variant:ident) => (
        impl<'a> Property<'a> {
            /// Get property value consuming self, without type conversion.
            pub fn $method_name(self) -> Result<$t, Self> {
                match self {
                    Property::$variant(v) => Ok(v),
                    s => Err(s),
                }
            }
        }
    );
    (vec, $t:ty, $method_name:ident, $variant:ident) => (
        impl<'a> Property<'a> {
            /// Get property value consuming self, without type conversion.
            pub fn $method_name(self) -> Result<Vec<$t>, Self> {
                match self {
                    Property::$variant(v) => Ok(v),
                    s => Err(s),
                }
            }
        }
    );
}

implement_getter_extract!(primitive, bool, extract_bool, Bool);
implement_getter_extract!(primitive, i16, extract_i16, I16);
implement_getter_extract!(primitive, i32, extract_i32, I32);
implement_getter_extract!(primitive, i64, extract_i64, I64);
implement_getter_extract!(primitive, f32, extract_f32, F32);
implement_getter_extract!(primitive, f64, extract_f64, F64);

implement_getter_extract!(vec, bool, extract_vec_bool, VecBool);
implement_getter_extract!(vec, i32, extract_vec_i32, VecI32);
implement_getter_extract!(vec, i64, extract_vec_i64, VecI64);
implement_getter_extract!(vec, f32, extract_vec_f32, VecF32);
implement_getter_extract!(vec, f64, extract_vec_f64, VecF64);

macro_rules! implement_property_value_into {
    ($t:ty, $method_name:ident, $variant:ident) => (
        impl<'a> Property<'a> {
            /// Get property value consuming self, without type conversion.
            pub fn $method_name(self) -> Result<$t, Self> {
                match self {
                    Property::$variant(v) => Ok(v),
                    s => Err(s),
                }
            }
        }
    );
}

impl<'a> Property<'a> {
    /// Get property value without consuming self, with type conversion.
    pub fn as_i32(&self) -> Option<i32> {
        match *self {
            Property::I16(v) => Some(v as i32),
            Property::I32(v) => Some(v),
            _ => None,
        }
    }

    /// Get property value without consuming self, with type conversion.
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Property::I16(v) => Some(v as i64),
            Property::I32(v) => Some(v as i64),
            Property::I64(v) => Some(v),
            _ => None,
        }
    }

    /// Get property value without consuming self, with type conversion.
    pub fn as_f32(&self) -> Option<f32> {
        match *self {
            Property::F32(v) => Some(v),
            Property::F64(v) => Some(v as f32),
            _ => None,
        }
    }

    /// Get property value without consuming self, with type conversion.
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Property::F32(v) => Some(v as f64),
            Property::F64(v) => Some(v),
            _ => None,
        }
    }

    /// Get property value without consuming self, with type conversion.
    pub fn as_vec_i64(&'a self) -> Option<Cow<'a, [i64]>> {
        match *self {
            Property::VecI32(ref v) => Some(Cow::Owned(v.iter().map(|&v| v as i64).collect::<Vec<_>>())),
            Property::VecI64(ref v) => Some(Cow::Borrowed(v)),
            _ => None,
        }
    }

    /// Get property value without consuming self, with type conversion.
    pub fn as_vec_f32(&'a self) -> Option<Cow<'a, [f32]>> {
        match *self {
            Property::VecF32(ref v) => Some(Cow::Borrowed(v)),
            Property::VecF64(ref v) => Some(Cow::Owned(v.iter().map(|&v| v as f32).collect::<Vec<_>>())),
            _ => None,
        }
    }

    /// Get property value without consuming self, with type conversion.
    pub fn as_vec_f64(&'a self) -> Option<Cow<'a, [f64]>> {
        match *self {
            Property::VecF32(ref v) => Some(Cow::Owned(v.iter().map(|&v| v as f64).collect::<Vec<_>>())),
            Property::VecF64(ref v) => Some(Cow::Borrowed(v)),
            _ => None,
        }
    }

    /// Get property value consuming self, with type conversion.
    pub fn into_vec_i64(self) -> Result<Vec<i64>, Self> {
        match self {
            Property::VecI32(v) => Ok(v.into_iter().map(|v| v as i64).collect::<Vec<_>>()),
            Property::VecI64(v) => Ok(v),
            s => Err(s),
        }
    }

    /// Get property value consuming self, with type conversion.
    pub fn into_vec_f32(self) -> Result<Vec<f32>, Self> {
        match self {
            Property::VecF32(v) => Ok(v),
            Property::VecF64(v) => Ok(v.into_iter().map(|v| v as f32).collect::<Vec<_>>()),
            s => Err(s),
        }
    }

    /// Get property value consuming self, with type conversion.
    pub fn into_vec_f64(self) -> Result<Vec<f64>, Self> {
        match self {
            Property::VecF32(v) => Ok(v.into_iter().map(|v| v as f64).collect::<Vec<_>>()),
            Property::VecF64(v) => Ok(v),
            s => Err(s),
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
