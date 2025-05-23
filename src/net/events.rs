use socket2::SockAddr;
use crate::packet::Packet;

/// Events that can occur in `laminar` and that will be pushed through the `event_receiver` returned by `Socket::bind`.
#[derive(Debug, PartialEq)]
pub enum SocketEvent {
    /// A packet was received from a client.
    Packet(Packet),
    /// A new connection has been established with a client. A connection is considered
    /// established whenever a packet has been both _sent_ and _received_ from the client.
    ///
    /// On the server—in order to receive a `Connect` event—you must respond to the first
    /// Packet from a new client.
    ///
    /// Clients are uniquely identified by the `ip:port` combination at this layer.
    Connect(SockAddr),
    /// The client has too many packets in flight and will be terminated.
    Overload(SockAddr),
    /// The client has been idling for longer than the `idle_connection_timeout` time.
    /// You can control the timeout in the config.
    Timeout(SockAddr),
    /// The established connection to a client has timed out.
    Disconnect(SockAddr),
}
