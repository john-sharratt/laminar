use std::convert::TryFrom;
use std::io::Cursor;

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use crate::PROTOCOL_VERSION;
use crate::error::Result;
use crate::net::constants::STANDARD_HEADER_SIZE;
use crate::packet::{ConnectionId, DeliveryGuarantee, EnumConverter, OrderingGuarantee, PacketType};

use super::{HeaderReader, HeaderWriter};

#[derive(Copy, Clone, Debug)]
/// This header will be included in each packet, and contains some basic information.
pub struct StandardHeader {
    protocol_version: u16,
    connection_id: ConnectionId,
    packet_type: PacketType,
    delivery_guarantee: DeliveryGuarantee,
    ordering_guarantee: OrderingGuarantee,
}

impl StandardHeader {
    /// Creates new header.
    pub fn new(
        delivery_guarantee: DeliveryGuarantee,
        ordering_guarantee: OrderingGuarantee,
        packet_type: PacketType,
        connection_seed: ConnectionId,
    ) -> Self {
        StandardHeader {
            protocol_version:PROTOCOL_VERSION,
            connection_id: connection_seed,
            delivery_guarantee,
            ordering_guarantee,
            packet_type,
        }
    }

    /// Returns the protocol version
    #[cfg(test)]
    pub fn protocol_version(&self) -> u16 {
        self.protocol_version
    }

    /// Returns the DeliveryGuarantee
    pub fn delivery_guarantee(&self) -> DeliveryGuarantee {
        self.delivery_guarantee
    }

    /// Returns the OrderingGuarantee
    pub fn ordering_guarantee(&self) -> OrderingGuarantee {
        self.ordering_guarantee
    }

    /// Returns the PacketType
    pub fn packet_type(&self) -> PacketType {
        self.packet_type
    }

    /// Returns the connection seed
    pub fn connection_id(&self) -> ConnectionId {
        self.connection_id
    }

    /// Returns true if the packet is a heartbeat packet, false otherwise
    pub fn is_heartbeat(&self) -> bool {
        self.packet_type == PacketType::Heartbeat
    }

    /// Returns true if the packet is a fragment, false if not
    pub fn is_fragment(&self) -> bool {
        self.packet_type == PacketType::Fragment
    }

    /// Checks if the protocol version in the packet is a valid version
    pub fn is_current_protocol(&self) -> bool {
        PROTOCOL_VERSION == self.protocol_version
    }
}

impl Default for StandardHeader {
    fn default() -> Self {
        StandardHeader::new(
            DeliveryGuarantee::Unreliable,
            OrderingGuarantee::None,
            PacketType::Packet,
            rand::random(),
        )
    }
}

impl HeaderWriter for StandardHeader {
    type Output = Result<()>;

    fn write(&self, buffer: &mut Vec<u8>) -> Self::Output {
        buffer.write_u16::<BigEndian>(self.protocol_version)?;
        buffer.write_u16::<BigEndian>(self.connection_id)?;
        buffer.write_u8(self.packet_type.to_u8())?;
        buffer.write_u8(self.delivery_guarantee.to_u8())?;
        buffer.write_u16::<BigEndian>(self.ordering_guarantee.to_u16())?;
        Ok(())
    }
}

impl HeaderReader for StandardHeader {
    type Header = Result<StandardHeader>;

    fn read(rdr: &mut Cursor<&[u8]>) -> Self::Header {
        let protocol_version = rdr.read_u16::<BigEndian>()?; /* protocol id */
        let connection_seed = rdr.read_u16::<BigEndian>()?;
        let packet_id = rdr.read_u8()?;
        let delivery_guarantee_id = rdr.read_u8()?;
        let order_guarantee_id = rdr.read_u16::<BigEndian>()?;

        let header = StandardHeader {
            protocol_version,
            packet_type: PacketType::try_from(packet_id)?,
            delivery_guarantee: DeliveryGuarantee::try_from(delivery_guarantee_id)?,
            ordering_guarantee: OrderingGuarantee::try_from(order_guarantee_id)?,
            connection_id: connection_seed,
        };

        Ok(header)
    }

    /// Returns the size of this header.
    fn size() -> usize {
        STANDARD_HEADER_SIZE
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::net::constants::STANDARD_HEADER_SIZE;
    use crate::packet::header::{HeaderReader, HeaderWriter, StandardHeader};
    use crate::packet::{DeliveryGuarantee, EnumConverter, OrderingGuarantee, PacketType};
    use crate::PROTOCOL_VERSION;

    #[test]
    fn serialize() {
        let mut b = Vec::new();
        let header = StandardHeader::new(
            DeliveryGuarantee::Unreliable,
            OrderingGuarantee::Sequenced(Some(8)),
            PacketType::Packet,
            17,
        );
        assert![header.write(&mut b).is_ok()];

        // [0 .. 3] protocol version
        assert_eq!([b[0], b[1]], 4u16.to_be_bytes());
        assert_eq!([b[2], b[3]], 17u16.to_be_bytes());
        assert_eq!(b[4], PacketType::Packet.to_u8());
        assert_eq!(b[5], DeliveryGuarantee::Unreliable.to_u8());
        assert_eq!([b[6], b[7]], OrderingGuarantee::Sequenced(Some(8)).to_u16().to_be_bytes());
    }

    #[test]
    fn serialize_then_deserialize() {
        let mut buffer = Vec::new();
        let header = StandardHeader::new(
            DeliveryGuarantee::Reliable,
            OrderingGuarantee::Ordered(Some(5)),
            PacketType::Packet,
            17,
        );
        assert![header.write(&mut buffer).is_ok()];

        let mut cursor = Cursor::new(buffer.as_slice());
        let header = StandardHeader::read(&mut cursor).unwrap();

        assert_eq!(header.protocol_version(), PROTOCOL_VERSION);
        assert_eq!(header.packet_type(), PacketType::Packet);
        assert_eq!(header.delivery_guarantee(), DeliveryGuarantee::Reliable);
        assert_eq!(header.connection_id(), 17);
        assert_eq!(
            header.ordering_guarantee(),
            OrderingGuarantee::Ordered(Some(5))
        );
    }

    #[test]
    fn deserialize() {
        let buffer = vec![0, 1, 0, 1, 1, 0, 3];

        let mut cursor = Cursor::new(buffer.as_slice());

        let header = StandardHeader::read(&mut cursor).unwrap();

        assert_eq!(header.protocol_version(), 1);
        assert_eq!(header.packet_type(), PacketType::Packet);
        assert_eq!(header.delivery_guarantee(), DeliveryGuarantee::Reliable);
        assert_eq!(header.connection_id(), 3);
        assert_eq!(
            header.ordering_guarantee(),
            OrderingGuarantee::Sequenced(None)
        );
    }

    #[test]
    fn size() {
        assert_eq!(8, STANDARD_HEADER_SIZE);
    }
}
