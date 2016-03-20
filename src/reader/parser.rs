//! Contains implementations of FBX parsers.

use std::io::Read;
use error::{Error, Result};
use event::{FbxEvent, FbxHeaderInfo};
use property::DelayedProperties;
use reader::ParserConfig;


#[macro_use]
mod macros {
    macro_rules! try_read_generic {
        ($read_expr:expr, $pos:expr, $size:expr) => ({
            let size = $size as usize;
            let ref mut pos: usize = $pos;
            *pos += size;
            match $read_expr {
                Ok(val) => val,
                Err(err) => {
                    *pos -= size;
                    return Err(Error::Io(err));
                },
            }
        })
    }
    macro_rules! try_read_u8 {
        ($reader:expr, $pos:expr) => ({
            use $crate::byteorder::ReadBytesExt;
            try_read_generic!($reader.read_u8(), $pos, 1)
        })
    }
    macro_rules! try_read_u32 {
        ($reader:expr, $pos:expr) => ({
            use $crate::byteorder::{LittleEndian, ReadBytesExt};
            try_read_generic!($reader.read_u32::<LittleEndian>(), $pos, 4)
        })
    }
    macro_rules! try_read_u64 {
        ($reader:expr, $pos:expr) => ({
            use $crate::byteorder::{LittleEndian, ReadBytesExt};
            try_read_generic!($reader.read_u64::<LittleEndian>(), $pos, 8)
        })
    }
    macro_rules! try_read_i16 {
        ($reader:expr, $pos:expr) => ({
            use $crate::byteorder::{LittleEndian, ReadBytesExt};
            try_read_generic!($reader.read_i16::<LittleEndian>(), $pos, 2)
        })
    }
    macro_rules! try_read_i32 {
        ($reader:expr, $pos:expr) => ({
            use $crate::byteorder::{LittleEndian, ReadBytesExt};
            try_read_generic!($reader.read_i32::<LittleEndian>(), $pos, 4)
        })
    }
    macro_rules! try_read_i64 {
        ($reader:expr, $pos:expr) => ({
            use $crate::byteorder::{LittleEndian, ReadBytesExt};
            try_read_generic!($reader.read_i64::<LittleEndian>(), $pos, 8)
        })
    }
    macro_rules! try_read_f32 {
        ($reader:expr, $pos:expr) => ({
            use $crate::byteorder::{LittleEndian, ReadBytesExt};
            try_read_generic!($reader.read_f32::<LittleEndian>(), $pos, 4)
        })
    }
    macro_rules! try_read_f64 {
        ($reader:expr, $pos:expr) => ({
            use $crate::byteorder::{LittleEndian, ReadBytesExt};
            try_read_generic!($reader.read_f64::<LittleEndian>(), $pos, 8)
        })
    }
    macro_rules! try_read_exact {
        ($reader:expr, $pos:expr, $buf:expr) => ({
            let buf: &mut _ = $buf;
            try_read_generic!($reader.read_exact(buf), $pos, buf.len());
        })
    }
    macro_rules! try_read_fixstr {
        ($reader:expr, $pos:expr, $len:expr) => ({
            let len = $len as usize;
            let mut buf = vec![0; len];
            try_read_generic!($reader.read_exact(&mut buf), $pos, len);
            try!(String::from_utf8(buf))
        })
    }
}


#[derive(Debug)]
enum State {
    ReadingMagic,
    ReadingNodes,
    SuccessfullyFinished,
    Error(Error),
}

pub struct Parser {
    config: ParserConfig,
    state: State,
    version: i32,
    pos: usize,
    end_offset_stack: Vec<u64>,
}

impl Parser {
    /// Constructs a parser.
    pub fn new(config: ParserConfig) -> Self {
        Parser {
            config: config,
            state: State::ReadingMagic,
            version: ::std::i32::MIN,
            pos: 0,
            end_offset_stack: vec![],
        }
    }

    /// Gets next `FbxEvent`.
    pub fn next<R: Read>(&mut self, reader: &mut R) -> Result<FbxEvent> {
        let result = match self.state {
            State::ReadingMagic => {
                self.magic_next(reader)
            },
            State::ReadingNodes => {
                self.nodes_next(reader)
            },
            State::SuccessfullyFinished => {
                return Ok(FbxEvent::EndFbx);
            },
            State::Error(ref err) => {
                return Err(err.clone());
            },
        };
        match result {
            Ok(FbxEvent::EndFbx) => {
                self.state = State::SuccessfullyFinished;
            },
            Err(ref err) => {
                self.state = State::Error(err.clone());
            },
            _ => {},
        }
        result
    }

    fn magic_next<R: Read>(&mut self, reader: &mut R) -> Result<FbxEvent> {
        {
            // 21 is the length of `b"Kaydara FBX Binary  \0"`.
            let mut magic = [0_u8; 21];
            try_read_exact!(reader, self.pos, &mut magic);
            if magic != *b"Kaydara FBX Binary  \0" {
                return Err(Error::InvalidMagic);
            }
        }
        {
            // "unknown but all observed files show these bytes",
            // see https://code.blender.org/2013/08/fbx-binary-file-format-specification/ .
            let mut buffer = [0_u8; 2];
            try_read_exact!(reader, self.pos, &mut buffer);
            if buffer != [0x1a, 0x00] {
                warn!("Expected [26, 0] right after magic binary, but got {:?}", buffer);
            }
        }
        let version = try_read_i32!(reader, self.pos);
        debug!("magic binary read, FBX binary (version={})", version);
        self.state = State::ReadingNodes;

        Ok(FbxEvent::StartFbx(FbxHeaderInfo {
            version: version,
        }))
    }

    fn nodes_next<R: Read>(&mut self, reader: &mut R) -> Result<FbxEvent> {
        // Check if the previously read node ends here.
        if let Some(&end_pos_top) = self.end_offset_stack.last() {
            if end_pos_top == self.pos as u64 {
                // Reached the end of previously read node.
                self.end_offset_stack.pop();
                return Ok(FbxEvent::EndNode);
            }
        }

        // Read a node record header.
        let node_record_header = try!(NodeRecordHeader::read_from(reader, &mut self.pos, self.version));
        if node_record_header.is_null_record() {
            // End of a node.
            return if let Some(expected_pos) = self.end_offset_stack.pop() {
                if self.pos == expected_pos as usize {
                    Ok(FbxEvent::EndNode)
                } else {
                    // Data is collapsed (the node doesn't end at expected position).
                    Err(Error::DataError(format!("Node does not end at expected position (expected {}, now at {})", expected_pos, self.pos)))
                }
            } else {
                // Reached end of all nodes.
                // (Extra NULL-record header is end marker of implicit root node.)
                // Footer with unknown contents follows.
                // TODO: Read footer.
                //       Files exported by official products or SDK have padding and their file
                //       sizes are multiple of 16, but some files exported by third-party apps
                //       (such as blender) does not.
                //       So it may be difficult to check if the footer is correct or wrong.
                // NOTE: There is the only thing known, the last 16 bytes of the data always seem
                //       to be `[0xf8, 0x5a, 0x8c, 0x6a, 0xde, 0xf5, 0xd9, 0x7e, 0xec, 0xe9, 0x0c,
                //       0xe3, 0x75, 0x8f, 0x29, 0x0b]`.
                Ok(FbxEvent::EndFbx)
            };
        } else {
            // Start of a node.
            self.end_offset_stack.push(node_record_header.end_offset);
        }

        // Read the node name.
        let name = try_read_fixstr!(reader, self.pos, node_record_header.name_len);

        // Read the properties.
        let properties = {
            let mut properties_raw = vec![0; node_record_header.property_byte_len as usize];
            try_read_exact!(reader, self.pos, &mut properties_raw);
            DelayedProperties::from_vec_u8(properties_raw, self.version, &self.config, node_record_header.num_properties as usize)
        };

        Ok(FbxEvent::StartNode {
            name: name,
            properties: properties,
        })
    }
}


/// A header of a node record.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct NodeRecordHeader {
    /// Position of the end of the node.
    end_offset: u64,
    /// Number of the properties the node has.
    num_properties: u64,
    /// Byte size of properties of the node in the FBX stream.
    property_byte_len: u64,
    /// Byte size of the node name.
    name_len: u8,
}

impl NodeRecordHeader {
    /// Constructs `NodeRecordHeader` from the given stream.
    pub fn read_from<R: Read>(reader: &mut R, pos: &mut usize, fbx_version: i32) -> Result<Self> {
        let (end_offset, num_properties, property_byte_len) = if fbx_version < 7500 {
            let end_offset = try_read_u32!(reader, *pos);
            let num_properties = try_read_u32!(reader, *pos);
            let property_byte_len = try_read_u32!(reader, *pos);
            (end_offset as u64, num_properties as u64, property_byte_len as u64)
        } else {
            let end_offset = try_read_u64!(reader, *pos);
            let num_properties = try_read_u64!(reader, *pos);
            let property_byte_len = try_read_u64!(reader, *pos);
            (end_offset, num_properties, property_byte_len)
        };
        let name_len = try_read_u8!(reader, *pos);

        Ok(NodeRecordHeader {
            end_offset: end_offset,
            num_properties: num_properties,
            property_byte_len: property_byte_len,
            name_len: name_len,
        })
    }

    /// Check whether the header indicates there are no more children.
    pub fn is_null_record(&self) -> bool {
        self.end_offset == 0
            && self.num_properties == 0
            && self.property_byte_len == 0
            && self.name_len == 0
    }
}
