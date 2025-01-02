use std::io::Cursor;

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use crate::packet::FragmentNumber;
use crate::{error::Result, packet::SequenceNumber};
use crate::net::constants::FRAGMENT_HEADER_SIZE;

use super::{HeaderReader, HeaderWriter};

#[derive(Copy, Clone, Debug)]
/// This header represents a fragmented packet header.
pub struct FragmentHeader {
    sequence: SequenceNumber,
    id: FragmentNumber,
    num_fragments: FragmentNumber,
}

impl FragmentHeader {
    /// Create new fragment with the given packet header.
    pub fn new(seq: SequenceNumber, id: FragmentNumber, num_fragments: FragmentNumber) -> Self {
        FragmentHeader {
            id,
            num_fragments,
            sequence: seq,
        }
    }

    /// Returns the id of this fragment.
    pub fn id(&self) -> FragmentNumber {
        self.id
    }

    /// Returns the sequence number of this fragment.
    pub fn sequence(&self) -> SequenceNumber {
        self.sequence
    }

    /// Returns the total number of fragments in the packet this fragment is part of.
    pub fn fragment_count(&self) -> FragmentNumber {
        self.num_fragments
    }
}

impl HeaderWriter for FragmentHeader {
    type Output = Result<()>;

    fn parse(&self, buffer: &mut Vec<u8>) -> Self::Output {
        buffer.write_u16::<BigEndian>(self.sequence)?;
        buffer.write_u8(self.id)?;
        buffer.write_u8(self.num_fragments)?;

        Ok(())
    }
}

impl HeaderReader for FragmentHeader {
    type Header = Result<FragmentHeader>;

    fn read(rdr: &mut Cursor<&[u8]>) -> Self::Header {
        let sequence = rdr.read_u16::<BigEndian>()?;
        let id = rdr.read_u8()?;
        let num_fragments = rdr.read_u8()?;

        let header = FragmentHeader {
            sequence,
            id,
            num_fragments,
        };

        Ok(header)
    }

    /// Returns the size of this header.
    fn size() -> usize {
        FRAGMENT_HEADER_SIZE
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::net::constants::FRAGMENT_HEADER_SIZE;
    use crate::packet::header::{FragmentHeader, HeaderReader, HeaderWriter};

    #[test]
    fn serialize() {
        let mut buffer = Vec::new();
        let header = FragmentHeader::new(1, 2, 3);
        assert![header.parse(&mut buffer).is_ok()];

        assert_eq!(buffer[1], 1);
        assert_eq!(buffer[2], 2);
        assert_eq!(buffer[3], 3);
    }

    #[test]
    fn deserialize() {
        let buffer = vec![0, 1, 2, 3];

        let mut cursor = Cursor::new(buffer.as_slice());

        let header = FragmentHeader::read(&mut cursor).unwrap();

        assert_eq!(header.sequence(), 1);
        assert_eq!(header.id(), 2);
        assert_eq!(header.fragment_count(), 3);
    }

    #[test]
    fn size() {
        assert_eq!(FragmentHeader::size(), FRAGMENT_HEADER_SIZE);
    }
}
