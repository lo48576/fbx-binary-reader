//! This crate provides pull parser for FBX binary.
//!
//! FBX data consists of generic node and node properties, and it requires interpretation to use as
//! 3D contents.
//! It is similar to relation of XML and COLLADA. COLLADA is represented using XML, but XML DOM is
//! difficult to use directly as COLLADA data.
//! Compare FBX to COLLADA, this crate is XML reader, not COLLADA importer.
//!
//! This crate is specialized to read FBX binary format fastly and would *NOT* implement FBX ASCII
//! reader or FBX writer.

extern crate byteorder;
extern crate flate2;
#[macro_use]
extern crate log;

pub use error::{Error, Result};
pub use event::{FbxEvent, FbxHeaderInfo};
pub use property::{DelayedProperties, Property, PropertiesIter};
pub use reader::{Events, EventReader};

pub mod error;
pub mod event;
pub mod property;
pub mod reader;
