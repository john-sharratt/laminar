use crate::packet::SequenceNumber;

#[derive(Clone)]
/// This contains the information required to reassemble fragments.
pub struct CongestionData<T: MomentInTime> {
    pub sequence: SequenceNumber,
    pub sending_time: T,
}

impl<T: MomentInTime> CongestionData<T> {
    pub fn new(sequence: SequenceNumber, sending_time: T) -> Self {
        CongestionData {
            sequence,
            sending_time,
        }
    }
}

impl<T: MomentInTime> Default for CongestionData<T> {
    fn default() -> Self {
        CongestionData {
            sequence: 0,
            sending_time: T::now(),
        }
    }
}
