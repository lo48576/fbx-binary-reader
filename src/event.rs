//! Contains a type for reader event.

use property::DelayedProperties;


/// Information in FBX file header.
#[derive(Debug, Clone)]
pub struct FbxHeaderInfo {
    /// FBX version.
    ///
    /// For FBX x.y, the value will be _x * 1000 + y * 100_.
    pub version: i32,
}

/// A node of an FBX input stream.
///
/// Items of this enum are emitted by [`reader::EventReader`](struct.EventReader.html).
#[derive(Debug)]
pub enum FbxEvent {
    /// Denotes start of FBX data.
    ///
    /// For Binary FBX, this item corresponds to magic binary.
    StartFbx(FbxHeaderInfo),
    /// Denotes end of FBX data.
    ///
    /// NOTE: Current implementation of Binary FBX parser does not read to the last byte of the FBX stream.
    EndFbx,
    /// Denotes beginning of a node.
    StartNode {
        /// Node name.
        name: String,
        /// Node properties.
        properties: DelayedProperties,
    },
    /// Denotes end of a node.
    EndNode,
}
