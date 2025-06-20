use std::io::Cursor;

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use crate::packet::AckFieldNumber;
use crate::{error::Result, packet::SequenceNumber};
use crate::net::constants::ACKED_PACKET_HEADER;

use super::{HeaderReader, HeaderWriter};

#[derive(Copy, Clone, Debug)]
/// This header provides reliability information.
pub struct AckedPacketHeader {
    /// This is the sequence number so that we can know where in the sequence of packages this packet belongs.
    pub seq: SequenceNumber,
    // This is the last acknowledged sequence number.
    ack_seq: SequenceNumber,
    // This is an bitfield of all last 32 acknowledged packages
    ack_field: AckFieldNumber,
}

impl AckedPacketHeader {
    /// When we compose packet headers, the local sequence becomes the sequence number of the packet, and the remote sequence becomes the ack.
    /// The ack bitfield is calculated by looking into a queue of up to 33 packets, containing sequence numbers in the range [remote sequence - 32, remote sequence].
    /// We set bit n (in [1,32]) in ack bits to 1 if the sequence number remote sequence - n is in the received queue.
    pub fn new(seq_num: SequenceNumber, last_seq: SequenceNumber, bit_field: AckFieldNumber) -> AckedPacketHeader {
        AckedPacketHeader {
            seq: seq_num,
            ack_seq: last_seq,
            ack_field: bit_field,
        }
    }

    /// Returns the sequence number from this packet.
    #[allow(dead_code)]
    pub fn sequence(&self) -> SequenceNumber {
        self.seq
    }

    /// Returns bit field of all last 32 acknowledged packages.
    pub fn ack_field(&self) -> AckFieldNumber {
        self.ack_field
    }

    /// Returns last acknowledged sequence number.
    pub fn ack_seq(&self) -> SequenceNumber {
        self.ack_seq
    }
}

impl HeaderWriter for AckedPacketHeader {
    type Output = Result<()>;

    fn write(&self, buffer: &mut Vec<u8>) -> Self::Output {
        buffer.write_u16::<BigEndian>(self.seq)?;
        buffer.write_u16::<BigEndian>(self.ack_seq)?;
        buffer.write_u32::<BigEndian>(self.ack_field)?;
        Ok(())
    }
}

impl HeaderReader for AckedPacketHeader {
    type Header = Result<AckedPacketHeader>;

    fn read(rdr: &mut Cursor<&[u8]>) -> Self::Header {
        let seq = rdr.read_u16::<BigEndian>()?;
        let ack_seq = rdr.read_u16::<BigEndian>()?;
        let ack_field = rdr.read_u32::<BigEndian>()?;

        Ok(AckedPacketHeader {
            seq,
            ack_seq,
            ack_field,
        })
    }

    fn size() -> usize {
        ACKED_PACKET_HEADER
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::net::constants::ACKED_PACKET_HEADER;
    use crate::packet::header::{AckedPacketHeader, HeaderReader, HeaderWriter};
    use crate::packet::{AckFieldNumber, SequenceNumber};

    #[test]
    fn serialize() {
        let mut buffer = Vec::new();
        let header = AckedPacketHeader::new(1, 2, 3);
        assert![header.write(&mut buffer).is_ok()];

        assert_eq!(buffer[size_of::<SequenceNumber>() - 1], 1);
        assert_eq!(buffer[(size_of::<SequenceNumber>() * 2) - 1], 2);
        assert_eq!(buffer[(size_of::<SequenceNumber>() * 2 + size_of::<AckFieldNumber>()) - 1], 3);
        assert_eq!(buffer.len(), AckedPacketHeader::size());
    }

    #[test]
    fn deserialize() {
        let buffer = vec![
            (1 as SequenceNumber).to_be_bytes().to_vec(),
            (2 as SequenceNumber).to_be_bytes().to_vec(),
            vec![0, 0, 0, 3]
        ].concat();

        let mut cursor = Cursor::new(buffer.as_slice());

        let header = AckedPacketHeader::read(&mut cursor).unwrap();

        assert_eq!(header.sequence(), 1);
        assert_eq!(header.ack_seq(), 2);
        assert_eq!(header.ack_field(), 3);
    }

    #[test]
    fn size() {
        assert_eq!(AckedPacketHeader::size(), ACKED_PACKET_HEADER);
    }
}
