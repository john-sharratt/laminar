use std::{self, fmt::Debug, io::IoSlice};
use coarsetime::Instant;
use socket2::SockAddr;

use crate::config::Config;

/// Allows connection to send packet, send event and get global configuration.
pub trait ConnectionMessenger<ReceiveEvent: Debug> {
    /// Returns global configuration.
    fn config(&self) -> &Config;

    /// Sends a connection event.
    fn send_event(&self, address: &SockAddr, event: ReceiveEvent);
    
    /// Sends a packet.
    fn send_packet(&self, address: &SockAddr, payload: &[u8]) -> std::io::Result<()>;
    
    /// Sends a packet with multiple buffers.
    fn send_packet_vectored(&mut self, address: &SockAddr, bufs: &[IoSlice<'_>]) -> std::io::Result<()>;
}

/// Returns an address of an event.
/// This is used by a `ConnectionManager`, because it doesn't know anything about connection events.
pub trait ConnectionEventAddress {
    /// Returns event address
    fn address(&self) -> SockAddr;
}

/// Allows to implement actual connection.
/// Defines a type of `Send` and `Receive` events, that will be used by a connection.
pub trait Connection: Debug {
    /// Defines a user event type.
    type SendEvent: Debug + ConnectionEventAddress;
    /// Defines a connection event type.
    type ReceiveEvent: Debug + ConnectionEventAddress;

    /// Creates new connection and initialize it by sending an connection event to the user.
    /// * messenger - allows to send packets and events, also provides a config.
    /// * address - defines a address that connection is associated with.
    /// * time - creation time, used by connection, so that it doesn't get dropped immediately or send heartbeat packet.
    fn create_connection(
        messenger: &impl ConnectionMessenger<Self::ReceiveEvent>,
        address: SockAddr,
        time: Instant,
    ) -> Self;

    /// Connections are considered established once they have both had both a send and a receive.
    fn is_established(&self) -> bool;

    /// Determines if the connection should be dropped due to its state.
    fn should_drop(
        &mut self,
        messenger: &impl ConnectionMessenger<Self::ReceiveEvent>,
        time: Instant,
    ) -> bool;

    /// Processes a received packet: parse it and emit an event.
    fn process_packet(
        &mut self,
        messenger: &mut impl ConnectionMessenger<Self::ReceiveEvent>,
        payload: &[u8],
        time: Instant,
    );

    /// Processes a received event and send a packet.
    fn process_event(
        &mut self,
        messenger: &impl ConnectionMessenger<Self::ReceiveEvent>,
        event: Self::SendEvent,
        time: Instant,
    );

    /// Processes various connection-related tasks: resend dropped packets, send heartbeat packet, etc...
    /// This function gets called frequently.
    fn update(
        &mut self,
        messenger: &impl ConnectionMessenger<Self::ReceiveEvent>,
        time: Instant,
    );
}
