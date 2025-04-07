use crate::{
    net::constants::{DEFAULT_ORDERING_STREAM, DEFAULT_SEQUENCING_STREAM},
    packet::{
        header::{
            AckedPacketHeader, ArrangingHeader, FragmentHeader, HeaderWriter, StandardHeader,
        },
        DeliveryGuarantee, OrderingGuarantee, PacketType,
    },
};

use super::{AckFieldNumber, ConnectionId, FragmentNumber, SequenceNumber, StreamNumber};

/// Builder that could be used to construct an outgoing laminar packet.
pub struct OutgoingPacketBuilder<'p> {
    header: Vec<u8>,
    payload: &'p [u8],
}

impl<'p> OutgoingPacketBuilder<'p> {
    /// Construct a new builder from the given `payload`.
    pub fn new(payload: &'p [u8]) -> OutgoingPacketBuilder<'p> {
        OutgoingPacketBuilder {
            header: Vec::new(),
            payload,
        }
    }

    /// Adds the `FragmentHeader` to the header.
    pub fn with_fragment_header(mut self, packet_seq: SequenceNumber, id: FragmentNumber, num_fragments: FragmentNumber) -> Self {
        let header = FragmentHeader::new(packet_seq, id, num_fragments);

        header
            .write(&mut self.header)
            .expect("Could not write fragment header to buffer");

        self
    }

    /// Adds the [`StandardHeader`](./headers/standard_header) to the header.
    pub fn with_default_header(
        mut self,
        packet_type: PacketType,
        delivery_guarantee: DeliveryGuarantee,
        ordering_guarantee: OrderingGuarantee,
        connection_seed: ConnectionId,
    ) -> Self {
        let header = StandardHeader::new(delivery_guarantee, ordering_guarantee, packet_type, connection_seed);
        header
            .write(&mut self.header)
            .expect("Could not write default header to buffer");

        self
    }

    /// Adds the [`AckedPacketHeader`](./headers/acked_packet_header) to the header.
    pub fn with_acknowledgment_header(
        mut self,
        seq_num: SequenceNumber,
        last_seq: SequenceNumber,
        bit_field: AckFieldNumber,
    ) -> Self {
        let header = AckedPacketHeader::new(seq_num, last_seq, bit_field);
        header
            .write(&mut self.header)
            .expect("Could not write acknowledgment header to buffer");

        self
    }

    /// Adds the [`ArrangingHeader`](./headers/arranging_header) if needed.
    ///
    /// - `arranging_id` = identifier for this packet that needs to be sequenced.
    /// - `stream_id` = stream on which this packet will be sequenced. If `None` than the a default stream will be used.
    pub fn with_sequencing_header(mut self, arranging_id: SequenceNumber, stream_id: Option<StreamNumber>) -> Self {
        let header =
            ArrangingHeader::new(arranging_id, stream_id.unwrap_or(DEFAULT_SEQUENCING_STREAM));

        header
            .write(&mut self.header)
            .expect("Could not write arranging header to buffer");

        self
    }

    /// Adds the [`ArrangingHeader`](./headers/arranging_header) if needed.
    ///
    /// - `arranging_id` = identifier for this packet that needs to be ordered.
    /// - `stream_id` = stream on which this packet will be ordered. If `None` than the a default stream will be used.
    pub fn with_ordering_header(mut self, arranging_id: SequenceNumber, stream_id: Option<StreamNumber>) -> Self {
        let header =
            ArrangingHeader::new(arranging_id, stream_id.unwrap_or(DEFAULT_ORDERING_STREAM));

        header
            .write(&mut self.header)
            .expect("Could not write arranging header to buffer");

        self
    }

    /// Constructs an `OutgoingPacket` from the contents constructed with this builder.
    pub fn build(self) -> OutgoingPacket<'p> {
        OutgoingPacket {
            header: self.header,
            payload: self.payload,
        }
    }
}

/// Packet that that contains data which is ready to be sent to a remote endpoint.
#[derive(Debug)]
pub struct OutgoingPacket<'p> {
    header: Vec<u8>,
    payload: &'p [u8],
}

impl<'p> OutgoingPacket<'p> {
    /// Return the contents of this packet; the content includes the header and payload bytes.
    ///
    /// # Remark
    /// - Until here we could use a reference to the outgoing data but here we need to do a hard copy.
    /// Because the header could vary in size but should be in front of the payload provided by the user.
    pub fn contents(&self) -> Box<[u8]> {
        [self.header.as_slice(), self.payload]
            .concat()
            .into_boxed_slice()
    }
}

#[cfg(test)]
mod tests {
    use crate::packet::{FragmentNumber, PacketType, SequenceNumber, StreamNumber};
    use crate::packet::{DeliveryGuarantee, OrderingGuarantee, OutgoingPacketBuilder};

    fn test_payload() -> Vec<u8> {
        b"test".to_vec()
    }

    #[test]
    fn assure_creation_fragment_header() {
        let payload = test_payload();

        let outgoing = OutgoingPacketBuilder::new(&payload)
            .with_fragment_header(0, 0, 0)
            .build();

        let expected: Vec<u8> = [
            (0 as SequenceNumber).to_be_bytes().to_vec(),
            (0 as FragmentNumber).to_be_bytes().to_vec(),
            (0 as FragmentNumber).to_be_bytes().to_vec(),
            test_payload()
        ].concat().to_vec();

        assert_eq!(outgoing.contents().to_vec(), expected);
    }

    #[test]
    fn assure_creation_arranging_header() {
        let payload = test_payload();

        let outgoing = OutgoingPacketBuilder::new(&payload)
            .with_sequencing_header(1, Some(2))
            .build();

        let mut buffer = Vec::new();
        buffer.extend_from_slice((1 as SequenceNumber).to_be_bytes().as_ref());
        buffer.extend_from_slice((2 as StreamNumber).to_be_bytes().as_ref());

        let expected: Vec<u8> = [buffer, test_payload()].concat().to_vec();

        assert_eq!(outgoing.contents().to_vec(), expected);
    }

    #[test]
    fn assure_creation_acknowledgment_header() {
        let payload = test_payload();

        let outgoing = OutgoingPacketBuilder::new(&payload)
            .with_acknowledgment_header(1, 2, 3)
            .build();

        let expected: Vec<u8> = [
                (1 as SequenceNumber).to_be_bytes().to_vec(),
                (2 as SequenceNumber).to_be_bytes().to_vec(),
                vec![0, 0, 0, 3],
                test_payload()
            ]
            .concat()
            .to_vec();

        assert_eq!(outgoing.contents().to_vec(), expected);
    }

    #[test]
    fn assure_creation_default_header() {
        let payload = test_payload();

        let outgoing = OutgoingPacketBuilder::new(&payload)
            .with_default_header(
                PacketType::Packet,
                DeliveryGuarantee::Reliable,
                OrderingGuarantee::Sequenced(None),
                0,
            )
            .build();

        let expected: Vec<u8> = [vec![0, 1, 1], test_payload()].concat().to_vec();

        assert_eq!(
            outgoing.contents()[2..outgoing.contents().len()].to_vec(),
            expected
        );
    }
}
