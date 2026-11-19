use crate::{
    config::Config,
    error::{FragmentErrorKind, Result},
    packet::header::{AckedPacketHeader, FragmentHeader},
    sequence_buffer::{ReassemblyData, SequenceBuffer},
};

/// Manages fragmentation of packets.
pub struct Fragmentation {
    fragments: SequenceBuffer<ReassemblyData>,
}

impl Fragmentation {
    /// Creates and returns a new Fragmentation
    pub fn new(config: &Config) -> Fragmentation {
        Fragmentation {
            fragments: SequenceBuffer::with_capacity(config.fragment_reassembly_buffer_size),
        }
    }

    /// Checks how many times a number fits into another number and will round up.
    ///
    /// For example we have two numbers:
    /// - number 1 = 4000;
    /// - number 2 = 1024;
    /// If you do it the easy way the answer will be 4000/1024 = 3.90625.
    /// But since we care about how how many whole times the number fits in we need the result 4.
    ///
    /// Note that when rust is rounding it is always rounding to zero (3.456 as u32 = 3)
    /// 1. calculate with modulo if `number 1` fits exactly in the `number 2`.
    /// 2. Divide `number 1` with `number 2` (this wil be rounded to zero by rust)
    /// 3. So in all cases we need to add 1 to get the right amount of fragments.
    ///
    /// lets take an example
    ///
    /// Calculate modules:
    /// - number 1 % number 2 = 928
    /// - this is bigger than 0 so remainder = 1
    ///
    /// Calculate how many times the `number 1` fits in `number 2`:
    /// - number 1 / number 2 = 3,90625 (this will be rounded to 3)
    /// - add remainder from above to 3 = 4.
    ///
    /// The above described method will figure out for all number how many times it fits into another number rounded up.
    ///
    /// So an example of dividing a packet of bytes we get these fragments:
    ///
    /// So for 4000 bytes we need 4 fragments
    /// [fragment: 1024] [fragment: 1024] [fragment: 1024] [fragment: 928]
    pub fn fragments_needed(payload_length: usize, fragment_size: usize) -> usize {
        let remainder = if payload_length % fragment_size > 0 {
            1
        } else {
            0
        };
        (payload_length / fragment_size) + remainder
    }

    /// Splits the given payload into fragments and write those fragments to the passed packet data.
    pub fn spit_into_fragments<'a>(payload: &'a [u8], config: &Config) -> Result<Vec<&'a [u8]>> {
        let mut fragments = Vec::new();

        let payload_length = payload.len();
        let num_fragments =
            // Safe cast max fragments is u8
            Fragmentation::fragments_needed(payload_length, config.fragment_size);

        if num_fragments > config.max_fragments as usize {
            return Err(FragmentErrorKind::ExceededMaxFragments.into());
        }

        for fragment_id in 0..num_fragments {
            // get start and end position of buffer
            let start_fragment_pos = usize::from(fragment_id) * config.fragment_size;
            let mut end_fragment_pos = (usize::from(fragment_id) + 1) * config.fragment_size;

            // If remaining buffer fits int one packet just set the end position to the length of the packet payload.
            if end_fragment_pos > payload_length {
                end_fragment_pos = payload_length;
            }

            // get specific slice of data for fragment
            let fragment_data = &payload[start_fragment_pos as usize..end_fragment_pos as usize];

            fragments.push(fragment_data);
        }

        Ok(fragments)
    }

    /// Reads fragment data and returns the complete packet when all its fragments are received.
    /// Each fragment's payload is kept by its id, so fragments put the packet together in order
    /// whatever order they arrive in.
    pub fn handle_fragment(
        &mut self,
        fragment_header: FragmentHeader,
        fragment_payload: &[u8],
        acked_header: Option<AckedPacketHeader>,
    ) -> Result<Option<(Vec<u8>, AckedPacketHeader)>> {
        if fragment_header.id() >= fragment_header.fragment_count() {
            return Err(FragmentErrorKind::TooManyFragments.into());
        }
        self.create_fragment_if_not_exists(fragment_header);

        let complete = {
            // get entry of previous received fragments
            let reassembly_data = match self.fragments.get_mut(fragment_header.sequence()) {
                Some(val) => val,
                None => return Err(FragmentErrorKind::CouldNotFindFragmentById.into()),
            };

            let part = &mut reassembly_data.parts[usize::from(fragment_header.id())];
            if part.is_some() {
                return Err(FragmentErrorKind::AlreadyProcessedFragment.into());
            }
            *part = Some(fragment_payload.to_vec());
            reassembly_data.num_fragments_received += 1;

            if let Some(acked_header) = acked_header {
                if reassembly_data.acked_header.is_none() {
                    reassembly_data.acked_header = Some(acked_header);
                } else {
                    return Err(FragmentErrorKind::MultipleAckHeaders.into());
                }
            }

            reassembly_data.num_fragments_received == reassembly_data.num_fragments_total
        };

        // if we received all fragments then remove entry and return the packet they make.
        if complete {
            let Some(mut reassembly_data) = self.fragments.remove(fragment_header.sequence()) else {
                return Err(FragmentErrorKind::CouldNotFindFragmentById.into());
            };
            let Some(acked_header) = reassembly_data.acked_header.take() else {
                return Err(FragmentErrorKind::MissingAckHeader.into());
            };
            return Ok(Some((reassembly_data.assembled(), acked_header)));
        }

        Ok(None)
    }

    /// Makes an entry for the fragment's packet if there is none. An entry of the same sequence
    /// with another count of fragments is what is left of an older packet that never completed:
    /// it is replaced.
    fn create_fragment_if_not_exists(&mut self, fragment_header: FragmentHeader) {
        let sequence = fragment_header.sequence();
        let stale = self
            .fragments
            .get(sequence)
            .is_some_and(|data| data.num_fragments_total != fragment_header.fragment_count());
        if stale || !self.fragments.exists(sequence) {
            let reassembly_data = ReassemblyData::new(fragment_header.fragment_count());
            self.fragments.insert(sequence, reassembly_data);
        }
    }
}

#[cfg(test)]
mod test {
    use super::Fragmentation;
    use crate::{
        config::Config,
        packet::header::{AckedPacketHeader, FragmentHeader},
    };

    #[test]
    pub fn expect_right_number_of_fragments() {
        let fragment_number = Fragmentation::fragments_needed(4000, 1024);
        let fragment_number1 = Fragmentation::fragments_needed(500, 1024);

        assert_eq!(fragment_number, 4);
        assert_eq!(fragment_number1, 1);
    }

    fn ack() -> AckedPacketHeader {
        AckedPacketHeader::new(7, 0, 0)
    }

    /// Feeds the fragments of `packet`, cut `size` bytes long, in the order of `ids`, the
    /// first fragment carrying the ack header as a sender's does; returns what completes.
    fn feed(packet: &[u8], size: usize, ids: &[usize]) -> Vec<Vec<u8>> {
        let config = Config {
            fragment_size: size,
            max_fragments: 18,
            ..Config::default()
        };
        let mut fragmentation = Fragmentation::new(&config);
        let pieces = Fragmentation::spit_into_fragments(packet, &config).unwrap();
        let count = pieces.len() as u8;
        let mut completed = Vec::new();
        for &id in ids {
            let header = FragmentHeader::new(7, id as u8, count);
            let acked = (id == 0).then(ack);
            let done = fragmentation.handle_fragment(header, pieces[id], acked).unwrap();
            completed.extend(done.map(|(bytes, _)| bytes));
        }
        completed
    }

    #[test]
    fn fragments_in_order_put_the_packet_back_together() {
        let packet: Vec<u8> = (0..25u8).collect();
        assert_eq!(feed(&packet, 10, &[0, 1, 2]), [packet]);
    }

    #[test]
    fn fragments_out_of_order_put_the_packet_back_together_in_order() {
        let packet: Vec<u8> = (0..25u8).collect();
        assert_eq!(feed(&packet, 10, &[1, 0, 2]), [packet.clone()]);
        assert_eq!(feed(&packet, 10, &[2, 1, 0]), [packet]);
    }

    #[test]
    fn a_packet_of_more_than_sixteen_fragments_is_put_back_together() {
        let packet: Vec<u8> = (0..175u8).collect();
        let ids: Vec<usize> = (0..18).rev().collect();
        assert_eq!(feed(&packet, 10, &ids), [packet]);
    }

    #[test]
    fn a_left_over_entry_of_another_count_is_replaced_by_the_new_packet() {
        let mut fragmentation = Fragmentation::new(&Config::default());
        // An older packet of sequence 7 in three fragments, of which one came.
        let old = FragmentHeader::new(7, 1, 3);
        assert!(fragmentation.handle_fragment(old, b"old", None).unwrap().is_none());
        // A new one of the same sequence in two.
        let first = FragmentHeader::new(7, 0, 2);
        assert!(fragmentation.handle_fragment(first, b"ab", Some(ack())).unwrap().is_none());
        let second = FragmentHeader::new(7, 1, 2);
        let done = fragmentation.handle_fragment(second, b"cd", None).unwrap();
        assert_eq!(done.map(|(bytes, _)| bytes), Some(b"abcd".to_vec()));
    }

    #[test]
    fn a_fragment_with_an_id_past_its_count_or_seen_twice_is_refused() {
        let mut fragmentation = Fragmentation::new(&Config::default());
        let past = FragmentHeader::new(7, 2, 2);
        assert!(fragmentation.handle_fragment(past, b"x", None).is_err());
        let first = FragmentHeader::new(7, 0, 2);
        assert!(fragmentation.handle_fragment(first, b"ab", Some(ack())).unwrap().is_none());
        assert!(fragmentation.handle_fragment(first, b"ab", Some(ack())).is_err());
    }
}
