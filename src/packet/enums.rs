use std::convert::TryFrom;

use crate::{
    error::{DecodingErrorKind, ErrorKind},
    packet::EnumConverter,
};

use super::StreamNumber;

/// Enum to specify how a packet should be delivered.
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Eq)]
pub enum DeliveryGuarantee {
    /// Packet may or may not be delivered
    Unreliable,
    /// Packet will be delivered
    Reliable,
}

impl EnumConverter for DeliveryGuarantee {
    type Enum = DeliveryGuarantee;

    /// Returns an integer value from `DeliveryGuarantee` enum.
    fn to_u8(&self) -> u8 {
        *self as u8
    }
}

impl TryFrom<u8> for DeliveryGuarantee {
    type Error = ErrorKind;
    /// Gets the `DeliveryGuarantee` enum instance from integer value.
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(DeliveryGuarantee::Unreliable),
            1 => Ok(DeliveryGuarantee::Reliable),
            _ => Err(ErrorKind::DecodingError(
                DecodingErrorKind::DeliveryGuarantee,
            )),
        }
    }
}

/// Enum to specify how a packet should be arranged.
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Eq, Default)]
pub enum OrderingGuarantee {
    /// No arranging will be done.
    #[default]
    None,
    /// Packets will be arranged in sequence.
    Sequenced(Option<StreamNumber>),
    /// Packets will be arranged in order.
    Ordered(Option<StreamNumber>),
}

impl OrderingGuarantee {
    /// Returns the integer value from `OrderingGuarantee` enum.
    pub fn to_u16(&self) -> u16 {
        match self {
            OrderingGuarantee::None => u16::from_be_bytes([0, 0]),
            OrderingGuarantee::Sequenced(None) => u16::from_be_bytes([1, 0]),
            OrderingGuarantee::Ordered(None) => u16::from_be_bytes([2, 0]),
            OrderingGuarantee::Sequenced(Some(s)) => u16::from_be_bytes([3, *s]),
            OrderingGuarantee::Ordered(Some(s)) => u16::from_be_bytes([4, *s]),
        }
    }
}

impl TryFrom<u16> for OrderingGuarantee {
    type Error = ErrorKind;
    /// Returns the `OrderingGuarantee` enum instance from integer value.
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        let value = value.to_be_bytes();
        match (value[0], value[1]) {
            (0, 0) => Ok(OrderingGuarantee::None),
            (1, 0) => Ok(OrderingGuarantee::Sequenced(None)),
            (2, 0) => Ok(OrderingGuarantee::Ordered(None)),
            (3, s) => Ok(OrderingGuarantee::Sequenced(Some(s))),
            (4, s) => Ok(OrderingGuarantee::Ordered(Some(s))),
            (_, _) => Err(ErrorKind::DecodingError(
                DecodingErrorKind::OrderingGuarantee,
            )),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialOrd, PartialEq)]
/// Id to identify a certain packet type.
pub enum PacketType {
    /// Full packet that is not fragmented
    Packet = 0,
    /// Fragment of a full packet
    Fragment = 1,
    /// Heartbeat packet
    Heartbeat = 2,
}

impl EnumConverter for PacketType {
    type Enum = PacketType;

    fn to_u8(&self) -> u8 {
        *self as u8
    }
}

impl TryFrom<u8> for PacketType {
    type Error = ErrorKind;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(PacketType::Packet),
            1 => Ok(PacketType::Fragment),
            2 => Ok(PacketType::Heartbeat),
            _ => Err(ErrorKind::DecodingError(DecodingErrorKind::PacketType)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::TryFrom;

    use crate::packet::{
        enums::{DeliveryGuarantee, OrderingGuarantee, PacketType},
        EnumConverter,
    };

    #[test]
    fn assure_parsing_ordering_guarantee() {
        let none = OrderingGuarantee::None;
        let ordered = OrderingGuarantee::Ordered(None);
        let sequenced = OrderingGuarantee::Sequenced(None);
        let ordered_with_stream = OrderingGuarantee::Ordered(Some(1));
        let sequenced_with_stream = OrderingGuarantee::Sequenced(Some(1));

        assert_eq!(
            OrderingGuarantee::None,
            OrderingGuarantee::try_from(none.to_u16()).unwrap()
        );
        assert_eq!(
            OrderingGuarantee::Ordered(None),
            OrderingGuarantee::try_from(ordered.to_u16()).unwrap()
        );
        assert_eq!(
            OrderingGuarantee::Sequenced(None),
            OrderingGuarantee::try_from(sequenced.to_u16()).unwrap()
        );
        assert_eq!(
            OrderingGuarantee::Ordered(Some(1)),
            OrderingGuarantee::try_from(ordered_with_stream.to_u16()).unwrap()
        );
        assert_eq!(
            OrderingGuarantee::Sequenced(Some(1)),
            OrderingGuarantee::try_from(sequenced_with_stream.to_u16()).unwrap()
        );
    }

    #[test]
    fn assure_parsing_delivery_guarantee() {
        let unreliable = DeliveryGuarantee::Unreliable;
        let reliable = DeliveryGuarantee::Reliable;
        assert_eq!(
            DeliveryGuarantee::Unreliable,
            DeliveryGuarantee::try_from(unreliable.to_u8()).unwrap()
        );
        assert_eq!(
            DeliveryGuarantee::Reliable,
            DeliveryGuarantee::try_from(reliable.to_u8()).unwrap()
        )
    }

    #[test]
    fn assure_parsing_packet_type() {
        let packet = PacketType::Packet;
        let fragment = PacketType::Fragment;
        let heartbeat = PacketType::Heartbeat;
        assert_eq!(
            PacketType::Packet,
            PacketType::try_from(packet.to_u8()).unwrap()
        );
        assert_eq!(
            PacketType::Fragment,
            PacketType::try_from(fragment.to_u8()).unwrap()
        );
        assert_eq!(
            PacketType::Heartbeat,
            PacketType::try_from(heartbeat.to_u8()).unwrap()
        );
    }
}
