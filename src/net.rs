//! This module provides the logic between the low-level abstract types and the types that the user will be interacting with.
//! You can think of the socket, connection management, congestion control.

pub use self::connection::{Connection, ConnectionEventAddress, ConnectionMessenger, MomentInTime};
pub use self::connection_manager::{
    debug_set_packet_delay, debug_set_packet_loss, debug_get_packet_delay, debug_get_packet_loss, ConnectionManager, ConnectionSender,
    DatagramSocket, DatagramSocketReceiver, DatagramSocketSender,
};
pub use self::events::SocketEvent;
pub use self::link_conditioner::LinkConditioner;
pub use self::socket::{Socket, SocketRx, SocketTx};
pub use self::virtual_connection::VirtualConnection;

mod connection;
mod connection_impl;
mod connection_manager;
mod events;
mod link_conditioner;
mod socket;
mod virtual_connection;

pub mod constants;
