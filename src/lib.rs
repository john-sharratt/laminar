//! Laminar is an application-level transport protocol which provides configurable reliability and ordering guarantees built on top of UDP.
//! It focuses on fast-paced fps-games and provides a lightweight, message-based interface.
//!
//! Laminar was designed to be used within the [Amethyst][amethyst] game engine but is usable without it.
//!
//! [amethyst]: https://github.com/amethyst/amethyst
//!
//! # Concepts
//!
//! This library is loosely based off of [Gaffer on Games][gog] and has features similar to RakNet, Steam Socket, and netcode.io.
//! The idea is to provide a native Rust low-level UDP-protocol which supports the use of cases of video games that require multiplayer features.
//! The library itself provides a few low-level types of packets that provide different types of guarantees. The most
//! basic are unreliable and reliable packets. Ordering, sequencing can be done on multiple streams.
//! For more information, read the projects [README.md][readme], [book][book], [docs][docs] or [examples][examples].
//!
//! [gog]: https://gafferongames.com/
//! [readme]: https://github.com/amethyst/laminar/blob/master/README.md
//! [book]: https://github.com/amethyst/laminar/tree/master/docs/md_book
//! [docs]: https://docs.rs/laminar/
//! [examples]: https://github.com/amethyst/laminar/tree/master/examples

#![deny(
    rust_2018_compatibility,
    rust_2018_idioms,
    future_incompatible,
    nonstandard_style,
    unused
)]
#![warn(missing_docs)]
#![allow(clippy::trivially_copy_pass_by_ref)]

pub use self::config::Config;
pub use self::error::{ErrorKind, Result};
pub use self::net::{
    constants::PROTOCOL_VERSION, debug_set_packet_delay, debug_set_packet_loss, debug_get_packet_delay, debug_get_packet_loss, debug_get_packet_recv, reset_debug_packet_recv, debug_get_packet_send, reset_debug_packet_send, Connection,
    ConnectionManager, ConnectionMessenger, ConnectionSender, DatagramSocket,
    DatagramSocketReceiver, DatagramSocketSender, LinkConditioner, MomentInTime, Socket,
    SocketEvent, SocketRx, SocketTx, VirtualConnection,
};
pub use self::packet::{DeliveryGuarantee, OrderingGuarantee, Packet};
#[cfg(feature = "tester")]
pub use self::throughput::ThroughputMonitoring;
pub use coarsetime;
pub use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError};
pub use packet::{FragmentNumber, SequenceNumber, StreamNumber};
pub use socket2;

mod config;
mod either;
mod error;
mod infrastructure;
mod net;
mod packet;
mod sequence_buffer;

#[cfg(feature = "tester")]
mod throughput;

/// Test utilities.
#[cfg(test)]
pub mod test_utils;
