use std::io::Cursor;
use bytes::BytesMut;

// Trait for reading/writing static frames from/to the network.
pub trait Frame<T> {
    /// Returns a parsed frame or `None` if it was incomplete.
    fn deserialize(src: &mut Cursor<&BytesMut>) -> Option<T>;
    /// Returns the bytes representation of the frame.
    fn serialize(&self) -> BytesMut;
}

// Specifies how to read/write dynamic frames from/to the network.
pub struct FrameFmt {
    // TODO: json spec for a single frame
}

impl FrameFmt {
    pub fn new() -> FrameFmt {
        FrameFmt {
            // TODO
        }
    }

    /// Returns a bytes representation of our frame that is suitable for writing
    /// out to the network.
    pub fn serialize(&self) -> BytesMut {
        todo!()
    }

    /// If a frame could be fully parsed from `src` according to our format,
    /// then we return the bytes that our format indicates that we should return
    /// (which may be a subset of the bytes that were read from `src`). If the
    /// full frame is not yet fully available in `src` according to our format,
    /// then we return None.
    pub fn deserialize(
        &self,
        src: &mut Cursor<&BytesMut>,
    ) -> Option<BytesMut> {
        todo!()
    }
}
