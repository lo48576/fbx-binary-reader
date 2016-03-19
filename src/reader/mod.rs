//! Contains interface for a pull-based (StAX-like) FBX parser.

use std::io::Read;
use error::Result;
use event::FbxEvent;

mod parser;


/// A wrapper around an `std::io::Read` instance which provides pull-based FBX parsing.
pub struct EventReader<R: Read> {
    source: R,
    parser: parser::Parser,
}

impl<R: Read> EventReader<R> {
    /// Creates a new reader, consuming the given stream.
    pub fn new(source: R) -> Self {
        EventReader {
            source: source,
            parser: parser::Parser::new(ParserConfig::new()),
        }
    }

    /// Creates a new reader with provided configuration, consuming the given stream.
    pub fn new_with_config(source: R, config: ParserConfig) -> Self {
        EventReader {
            source: source,
            parser: parser::Parser::new(config),
        }
    }

    /// Pulls and returns next FBX event from the stream.
    pub fn next(&mut self) -> Result<FbxEvent> {
        self.parser.next(&mut self.source)
    }
}

impl <R: Read> IntoIterator for EventReader<R> {
    type Item = Result<FbxEvent>;
    type IntoIter = Events<R>;

    /// Consumes `EventReader` and returns an iterator (`Events`) over it.
    fn into_iter(self) -> Events<R> {
        Events {
            reader: self,
            finished: false,
        }
    }
}

/// An iterator over FBX events created from some type implementing `Read`.
///
/// When the next event is [`reader::error::Error`](struct.Error.html) or
/// [`reader::FbxEvent::EndFbx`](enum.FbxEvent.html) then it will be returned
/// by the iterator once, and then it will stop producing events.
pub struct Events<R: Read> {
    reader: EventReader<R>,
    finished: bool,
}

impl<R: Read> Events<R> {
    /// Returns internal `EventReader`.
    #[allow(dead_code)]
    fn into_inner(self) -> EventReader<R> {
        self.reader
    }
}

impl<R: Read> Iterator for Events<R> {
    type Item = Result<FbxEvent>;

    fn next(&mut self) -> Option<Result<FbxEvent>> {
        if self.finished {
            None
        } else {
            let ev = self.reader.next();
            match ev {
                Ok(FbxEvent::EndFbx) | Err(_) => self.finished = true,
                _ => {}
            }
            Some(ev)
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ParserConfig;

impl ParserConfig {
    /// Creates a new config with default options.
    pub fn new() -> Self {
        Default::default()
    }

    /// Creates an FBX reader with this configuration.
    pub fn create_reader<R: Read>(self, source: R) -> EventReader<R> {
        EventReader::new_with_config(source, self)
    }
}
