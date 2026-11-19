use crate::packet::header::AckedPacketHeader;
use crate::packet::FragmentNumber;

#[derive(Clone, Default)]
/// This contains the information required to reassemble fragments: each fragment's payload is
/// kept by its id, so the packet is put back together in order whatever order they came in.
pub struct ReassemblyData {
    pub num_fragments_received: FragmentNumber,
    pub num_fragments_total: FragmentNumber,
    /// Each fragment's payload by its id; none until it has arrived.
    pub parts: Vec<Option<Vec<u8>>>,
    pub acked_header: Option<AckedPacketHeader>,
}

impl ReassemblyData {
    pub fn new(num_fragments_total: FragmentNumber) -> Self {
        Self {
            num_fragments_received: 0,
            num_fragments_total,
            parts: vec![None; usize::from(num_fragments_total)],
            acked_header: None,
        }
    }

    /// The packet: every fragment's payload, in the order of their ids.
    pub fn assembled(&self) -> Vec<u8> {
        let mut packet = Vec::with_capacity(self.parts.iter().flatten().map(Vec::len).sum());
        for part in self.parts.iter().flatten() {
            packet.extend_from_slice(part);
        }
        packet
    }
}
