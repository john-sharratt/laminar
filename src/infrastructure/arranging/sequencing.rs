//! Module with logic for arranging items in-sequence on multiple streams.
//!
//! "_Sequencing is the process of only caring about the newest items._"
//!
//! With sequencing, we only care about the newest items. When old items arrive we just toss them away.
//!
//! Example: sequence `1,3,2,5,4` will result into `1,3,5`.
//!
//! # Remarks
//! - See [super-module](../index.html) description for more details.

use std::{collections::HashMap, marker::PhantomData};

use linked_hash_set::LinkedHashSet;

use crate::packet::{SequenceNumber, StreamNumber, SEQUENCE_MID};

use super::{Arranging, ArrangingSystem};

/// A sequencing system that can arrange items in sequence across different streams.
///
/// Checkout [`SequencingStream`](./struct.SequencingStream.html), or module description for more details.
///
/// # Remarks
/// - See [super-module](../index.html) for more information about streams.
pub struct SequencingSystem<T> {
    // '[HashMap]' with streams on which items can be arranged in-sequence.
    streams: HashMap<StreamNumber, SequencingStream<T>>,
    // the maximum number of holes that can be stored before it starts dropped holes.
    max_holes: usize,
}

impl<T> SequencingSystem<T> {
    /// Constructs a new [`SequencingSystem`](./struct.SequencingSystem.html).
    pub fn new(max_holes: usize) -> SequencingSystem<T> {
        SequencingSystem {
            streams: HashMap::with_capacity(32),
            max_holes
        }
    }

    /// Resets the sequencing system after the connection was reset.
    pub fn reset(&mut self) {
        self.streams.clear();
    }
}

impl<T> ArrangingSystem for SequencingSystem<T> {
    type Stream = SequencingStream<T>;

    /// Returns the number of sequencing streams currently created.
    fn stream_count(&self) -> usize {
        self.streams.len()
    }

    /// Tries to get an [`SequencingStream`](./struct.SequencingStream.html) by `stream_id`.
    /// When the stream does not exist, it will be inserted by the given `stream_id` and returned.
    fn get_or_create_stream(&mut self, stream_id: StreamNumber) -> &mut Self::Stream {
        self.streams
            .entry(stream_id)
            .or_insert_with(|| SequencingStream::new(stream_id, self.max_holes))
    }
}

/// A stream on which items will be arranged in-sequence.
///
/// # Algorithm
///
/// With every sequencing operation an `top_index` is given.
///
/// There are two scenarios that are important to us.
/// 1. `incoming_index` >= `top_index`.
/// This item is the newest or newer than the last one we have seen.
/// Because of that we should return it back to the user.
/// 2. `incoming_index` < `top_index`.
/// This item is older than the newest item we have seen so far.
/// Since we don't care about old items we can toss it a way.
///
/// # Remarks
/// - See [super-module](../index.html) for more information about streams.
pub struct SequencingStream<T> {
    // the id of this stream.
    _stream_id: StreamNumber,
    // the highest seen item index.
    top_index: SequenceNumber,
    // holes occur when packets are received out of order.
    holes: LinkedHashSet<SequenceNumber>,
    // the maximum number of holes that can be stored before it
    // starts dropped holes.
    max_holes: usize,
    // Needs `PhantomData`, otherwise, it can't use a generic in the `Arranging` implementation because `T` is not constrained.
    phantom: PhantomData<T>,
    // unique identifier which should be used for ordering on an other stream e.g. the remote endpoint.
    unique_item_identifier: SequenceNumber,
}

impl<T> SequencingStream<T> {
    /// Constructs a new, empty '[SequencingStream](./struct.SequencingStream.html)'.
    ///
    /// The default stream will have a capacity of 32 items.
    pub fn new(stream_id: StreamNumber, max_holes: usize) -> SequencingStream<T> {
        SequencingStream {
            _stream_id: stream_id,
            top_index: 0,
            holes: LinkedHashSet::new(),
            max_holes,
            phantom: PhantomData,
            unique_item_identifier: 0,
        }
    }

    /// Returns the identifier of this stream.
    #[cfg(test)]
    pub fn stream_id(&self) -> StreamNumber {
        self._stream_id
    }

    /// Returns the unique identifier which should be used for ordering on an other stream e.g. the remote endpoint.
    pub fn new_item_identifier(&mut self) -> SequenceNumber {
        let id = self.unique_item_identifier;
        self.unique_item_identifier = self.unique_item_identifier.wrapping_add(1);
        id
    }
}

fn is_seq_within_half_window_from_start(start: SequenceNumber, incoming: SequenceNumber) -> bool {
    // check (with wrapping) if the incoming value lies within the next seq::max_value()/2 from
    // start.
    incoming.wrapping_sub(start) <= SEQUENCE_MID
}

impl<T> Arranging for SequencingStream<T> {
    type ArrangingItem = T;

    /// Arranges the given item based on a sequencing algorithm.
    ///
    /// With every sequencing operation an `top_index` is given.
    ///
    /// # Algorithm
    ///
    /// There are two scenarios that are important to us.
    /// 1. `incoming_index` >= `top_index`.
    /// This item is the newest or newer than the last one we have seen.
    /// Because of that we should return it back to the user.
    /// 2. `incoming_index` < `top_index`.
    /// This item is older than we the newest packet we have seen so far.
    /// Since we don't care about old items we can toss it a way.
    ///
    /// # Remark
    /// - All old packets will be tossed away.
    /// - None is returned when an old packet is received.
    fn arrange(
        &mut self,
        incoming_index: SequenceNumber,
        item: Self::ArrangingItem,
    ) -> Option<Self::ArrangingItem> {
        // if the incoming index is something in the future then we will return it.
        if is_seq_within_half_window_from_start(self.top_index, incoming_index) {            
            // remembering holes is an optional feature
            if self.max_holes > 0 {
                // any holes that are too old will be removed.
                while self.holes.front().map(|&x| is_seq_within_half_window_from_start(self.top_index, x)).unwrap_or(false) {
                    self.holes.pop_front();
                }
                // create any holes that need to be created due to receiving packets out of order.
                let mut tickets = self.max_holes;
                let mut hole = self.top_index.wrapping_add(1);
                while tickets > 0 && hole != incoming_index {
                    self.holes.insert(hole);
                    hole = hole.wrapping_add(1);
                    tickets = tickets.saturating_sub(1);
                }
                while self.holes.len() > self.max_holes {
                    self.holes.pop_front();
                }
            }

            // update the pointer to the incoming index.
            self.top_index = incoming_index;
            return Some(item);
        }

        // it might also be that this packet is plugging a hole for a previous packet that
        // was sent out of order.
        if self.holes.remove(&incoming_index) {
            return Some(item);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use crate::packet::{SequenceNumber, StreamNumber};

    use super::{Arranging, ArrangingSystem, SequencingSystem};

    #[derive(Debug, PartialEq, Clone)]
    struct Packet {
        pub sequence: SequenceNumber,
        pub ordering_stream: StreamNumber,
    }

    impl Packet {
        fn new(sequence: SequenceNumber, ordering_stream: StreamNumber) -> Packet {
            Packet {
                sequence,
                ordering_stream,
            }
        }
    }

    #[test]
    fn create_stream() {
        let mut system: SequencingSystem<Packet> = SequencingSystem::new(0);
        let stream = system.get_or_create_stream(1);

        assert_eq!(stream.stream_id(), 1);
    }

    #[test]
    fn create_existing_stream() {
        let mut system: SequencingSystem<Packet> = SequencingSystem::new(0);

        system.get_or_create_stream(1);
        let stream = system.get_or_create_stream(1);

        assert_eq!(stream.stream_id(), 1);
    }

    /// asserts that the given collection, on the left, should result - after it is sequenced - into the given collection, on the right.
    macro_rules! assert_sequence {
        ( [$( $x:expr ),*], [$( $y:expr),*], $stream_id:expr) => {
            {
                // initialize vector of given range on the left.
                let before = [$($x,)*];

                // initialize vector of given range on the right.
                let after = [$($y,)*];

                // create system to handle sequenced packets.
                let mut sequence_system = SequencingSystem::<Packet>::new(0);

                // get stream '1' to process the sequenced packets on.
                let stream = sequence_system.get_or_create_stream(1);

                // get packets arranged in sequence.
                let sequenced_packets: Vec<_> = before.into_iter()
                    .filter_map(|seq| stream.arrange(seq, Packet::new(seq, $stream_id)) // filter sequenced packets
                        .map(|p| p.sequence))
                    .collect();

               // assert if the expected range of the given numbers equals to the processed range which is in sequence.
               assert_eq!(after.to_vec(), sequenced_packets);
            }
        };
    }

    // This will assert a bunch of ranges to a correct sequenced range.
    #[test]
    fn can_sequence() {
        assert_sequence!([1, 3, 5, 4, 2], [1, 3, 5], 1);
        assert_sequence!([1, 5, 4, 3, 2], [1, 5], 1);
        assert_sequence!([5, 3, 4, 2, 1], [5], 1);
        assert_sequence!([4, 3, 2, 1, 5], [4, 5], 1);
        assert_sequence!([2, 1, 4, 3, 5], [2, 4, 5], 1);
        assert_sequence!([5, 2, 1, 4, 3], [5], 1);
        assert_sequence!([3, 2, 4, 1, 5], [3, 4, 5], 1);
    }

    // This will assert a bunch of ranges to a correct sequenced range.
    #[test]
    fn sequence_on_multiple_streams() {
        assert_sequence!([1, 3, 5, 4, 2], [1, 3, 5], 1);
        assert_sequence!([1, 5, 4, 3, 2], [1, 5], 2);
        assert_sequence!([5, 3, 4, 2, 1], [5], 3);
        assert_sequence!([4, 3, 2, 1, 5], [4, 5], 4);
        assert_sequence!([2, 1, 4, 3, 5], [2, 4, 5], 5);
        assert_sequence!([5, 2, 1, 4, 3], [5], 6);
        assert_sequence!([3, 2, 4, 1, 5], [3, 4, 5], 7);
    }
}
