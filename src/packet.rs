//! This module provides all the logic around the packet, such as reading, parsing, and constructing headers.

pub use self::enums::{DeliveryGuarantee, OrderingGuarantee, PacketType};
pub use self::outgoing::{OutgoingPacket, OutgoingPacketBuilder};
pub use self::packet_reader::PacketReader;
pub use self::packet_structure::{Packet, PacketInfo};
pub use self::process_result::{IncomingPackets, OutgoingPackets};

pub mod header;

mod enums;
mod outgoing;
mod packet_reader;
mod packet_structure;
mod process_result;

/// The stream of ordered/sequential packets for a specific connection
pub type StreamNumber = u8;
/// The fragment number of a fragmented packet
pub type FragmentNumber = u8;
/// The sequence number of a packet
pub type SequenceNumber = u16;
pub type AckFieldNumber = u32;
pub type ConnectionId = u16;

pub const SEQUENCE_MID: SequenceNumber = ((SequenceNumber::MAX - 1) / 2 ) + 1;

pub trait EnumConverter {
    type Enum;

    fn to_u8(&self) -> u8;
}
