use std::time::Duration;

use crate::packet::{AckFieldNumber, FragmentNumber, SequenceNumber, StreamNumber};

/// The size of the fragment header.
pub const FRAGMENT_HEADER_SIZE: usize =
    size_of::<SequenceNumber>() + size_of::<FragmentNumber>() * 2;
/// The size of the acknowledgment header.
pub const ACKED_PACKET_HEADER: usize =
    size_of::<SequenceNumber>() * 2 + size_of::<AckFieldNumber>();
/// The size of the arranging header.
pub const ARRANGING_PACKET_HEADER: usize = size_of::<SequenceNumber>() + size_of::<StreamNumber>();
/// The size of the standard header.
pub const STANDARD_HEADER_SIZE: usize = 5;
/// The ordering stream that will be used to order on if none was specified.
pub const DEFAULT_ORDERING_STREAM: StreamNumber = StreamNumber::MAX;
/// The sequencing stream that will be used to sequence packets on if none was specified.
pub const DEFAULT_SEQUENCING_STREAM: StreamNumber = StreamNumber::MAX;
/// Default maximal number of fragments to size.
pub const MAX_FRAGMENTS_DEFAULT: FragmentNumber = 16;
/// Default maximal size of each fragment.
pub const FRAGMENT_SIZE_DEFAULT: usize = 1024;
/// Defaultamount of time to resend a packet if no acknowledgment has been received.
pub const DEFAULT_RESEND_AFTER: Duration = Duration::from_millis(500);
/// Defaultamount of time to resend a packet if no acknowledgment has been received.
pub const DEFAULT_FAST_RESEND_AFTER: Duration = Duration::from_millis(100);
/// Maximum transmission unit of the payload.
///
/// Derived from ethernet_mtu - ipv6_header_size - udp_header_size - packet header size
///       1452 = 1500         - 40               - 8               - 8
///
/// This is not strictly guaranteed -- there may be less room in an ethernet frame than this due to
/// variability in ipv6 header size.
pub const DEFAULT_MTU: u16 = 1452;
/// This is the current protocol version.
///
/// Incremental monolithic protocol number.
pub const PROTOCOL_VERSION: u16 = 3;
