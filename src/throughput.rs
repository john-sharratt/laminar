use std::fmt::{self, Debug, Display};
use std::time::Duration;

/// Entry for throughput monitor with measured information.
#[derive(Debug)]
struct ThroughputEntry<T: MomentInTime> {
    measured_throughput: u32,
    _start: Instant,
}

impl ThroughputEntry {
    /// Constructs a new throughput entry.
    pub fn new(measured_throughput: u32, time: T) -> ThroughputEntry {
        ThroughputEntry {
            measured_throughput,
            _start: time,
        }
    }
}

/// Helper to monitor throughput.
///
/// Throughput is calculated at some duration.
/// For each duration an entry is created to keep track of the history of throughput.
///
/// With this type you can calculate the average or get the total and last measured throughput.
pub struct ThroughputMonitoring<T: MomentInTime> {
    throughput_duration: Duration,
    timer: T,
    current_throughput: u32,
    measured_throughput: Vec<ThroughputEntry>,
}

impl<T: MomentInTime> ThroughputMonitoring<T> {
    /// Constructs a new instance of `ThroughputMonitoring`.
    pub fn new(throughput_duration: Duration) -> ThroughputMonitoring {
        ThroughputMonitoring {
            throughput_duration,
            timer: T::now(),
            current_throughput: 0,
            measured_throughput: Vec::new(),
        }
    }

    /// Increases the throughput by one, when the `throughput_duration` has elapsed since the last call, then an throughput entry will be created.
    pub fn tick(&mut self) -> bool {
        if self.timer.elapsed() >= self.throughput_duration {
            self.measured_throughput
                .push(ThroughputEntry::new(self.current_throughput, self.timer));
            self.current_throughput = 0;
            self.timer = T::now();
            true
        } else {
            self.current_throughput += 1;
            false
        }
    }

    /// Returns the average throughput over all throughput up-till now.
    pub fn average(&self) -> u32 {
        if !self.measured_throughput.is_empty() {
            return self
                .measured_throughput
                .iter()
                .map(|x| x.measured_throughput)
                .sum::<u32>()
                / self.measured_throughput.len() as u32;
        }
        0
    }

    /// Reset the throughput history.
    pub fn reset(&mut self) {
        self.current_throughput = 0;
        self.measured_throughput.clear();
    }

    /// Returns the last measured throughput.
    pub fn last_throughput(&self) -> u32 {
        self.measured_throughput
            .last()
            .map(|x| x.measured_throughput)
            .unwrap_or(0)
    }

    /// Returns the totals measured throughput ticks.
    pub fn total_measured_ticks(&self) -> u32 {
        self.measured_throughput
            .iter()
            .map(|x| x.measured_throughput)
            .sum::<u32>()
            + self.current_throughput
    }
}

impl<T: MomentInTime> Debug for ThroughputMonitoring<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            f,
            "Current Throughput: {}, Elapsed Time: {:#?}, Average Throughput: {}",
            self.last_throughput(),
            self.timer.elapsed(),
            self.average()
        )
    }
}

impl<T: MomentInTime> Display for ThroughputMonitoring<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            f,
            "Current Throughput: {}, Elapsed Time: {:#?}, Average Throughput: {}",
            self.last_throughput(),
            self.timer.elapsed(),
            self.average()
        )
    }
}
