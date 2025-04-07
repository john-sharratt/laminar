use log::{debug, error, warn};
use socket2::SockAddr;

use crate::error::{ErrorKind, Result};
use crate::packet::{DeliveryGuarantee, OutgoingPackets, Packet, PacketInfo};

use super::connection::MomentInTime;
use super::{
    events::SocketEvent, Connection, ConnectionEventAddress, ConnectionMessenger, VirtualConnection,
};

/// Required by `ConnectionManager` to properly handle connection event.
impl ConnectionEventAddress for SocketEvent {
    /// Returns event address.
    fn address(&self) -> SockAddr {
        match self {
            SocketEvent::Packet(packet) => packet.addr().clone(),
            SocketEvent::Connect(addr) => addr.clone(),
            SocketEvent::Overload(addr) => addr.clone(),
            SocketEvent::Timeout(addr) => addr.clone(),
            SocketEvent::Disconnect(addr) => addr.clone(),
        }
    }
}

/// Required by `ConnectionManager` to properly handle user event.
impl ConnectionEventAddress for Packet {
    /// Returns event address.
    fn address(&self) -> SockAddr {
        self.addr().clone()
    }
}

impl MomentInTime for coarsetime::Instant {
    /// Returns a time in milliseconds.
    fn duration_since(&self, other: Self) -> std::time::Duration {
        self.duration_since(other).into()
    }

    /// Returns the current time in milliseconds.
    fn now() -> Self {
        coarsetime::Instant::now()
    }
}

impl<T: MomentInTime> Connection for VirtualConnection<T> {
    /// Defines a user event type.
    type SendEvent = Packet;
    /// Defines a connection event type.
    type ReceiveEvent = SocketEvent;
    /// Defines a moment in time.
    type Instant = T;

    /// Creates new connection and initialize it by sending an connection event to the user.
    /// * address - defines a address that connection is associated with.
    /// * time - creation time, used by connection, so that it doesn't get dropped immediately or send heartbeat packet.
    /// * initial_data - if initiated by remote host, this will hold that a packet data.
    fn create_connection(
        messenger: &impl ConnectionMessenger<Self::ReceiveEvent>,
        address: SockAddr,
        time: Self::Instant,
    ) -> VirtualConnection<Self::Instant> {
        VirtualConnection::new(address, messenger.config(), time)
    }

    ///  Connections are considered established once they both have had a send and a receive.
    fn is_established(&self) -> bool {
        self.is_established()
    }

    /// Determines if the given `Connection` should be dropped due to its state.
    fn should_drop(
        &mut self,
        messenger: &impl ConnectionMessenger<Self::ReceiveEvent>,
        time: Self::Instant,
    ) -> bool {
        let too_many = self.packets_in_flight() > messenger.config().max_packets_in_flight;
        let too_late = self.last_heard(time) >= messenger.config().idle_connection_timeout;
        let should_drop = too_many || too_late;
        if should_drop {
            if too_many {
                log::warn!("Connection overload for [{:?}]: {} packets in flight", &self.remote_address, self.packets_in_flight());
                messenger.send_event(
                    &self.remote_address,
                    SocketEvent::Overload(self.remote_address.clone()),
                );
            }
            if too_late {
                log::warn!("Connection timeout for [{:?}]: {}ms", &self.remote_address, self.last_heard(time).as_millis());
                messenger.send_event(
                    &self.remote_address,
                    SocketEvent::Timeout(self.remote_address.clone()),
                );
            }
            if self.is_established() {
                log::warn!("Connection dropped for [{:?}]: {}ms", &self.remote_address, self.last_heard(time).as_millis());
                messenger.send_event(
                    &self.remote_address,
                    SocketEvent::Disconnect(self.remote_address.clone()),
                );
            }
        }
        should_drop
    }

    /// Processes a received packet: parse it and emit an event.
    fn process_packet(
        &mut self,
        messenger: &mut impl ConnectionMessenger<Self::ReceiveEvent>,
        payload: &[u8],
        time: Self::Instant,
        should_reset: bool
    ) {
        if !payload.is_empty() {
            match self.process_incoming(payload, time) {
                Ok(packets) => {
                    if self.record_recv() {
                        messenger.send_event(
                            &self.remote_address,
                            SocketEvent::Connect(self.remote_address.clone()),
                        );
                    }

                    for (pck, _, conn_id) in packets {
                        if conn_id != self.connection_id && should_reset {
                            debug!("connection reset: {:?}", self.remote_address);
                            self.reset(conn_id);
                        }
                        messenger.send_event(&self.remote_address, SocketEvent::Packet(pck));
                    }
                }
                Err(err) => debug!("Error occured processing incomming packet: {}", err),
            }
        } else {
            warn!(
                "Error processing packet: {}",
                ErrorKind::ReceivedDataToShort
            );
        }
    }

    /// Processes a received event and send a packet.
    fn process_event(
        &mut self,
        messenger: &impl ConnectionMessenger<Self::ReceiveEvent>,
        event: Self::SendEvent,
        time: Self::Instant,
    ) {
        let addr = self.remote_address.clone();
        if self.record_send() {
            messenger.send_event(&addr, SocketEvent::Connect(addr.clone()));
        }

        send_packets(
            messenger,
            &addr,
            self.process_outgoing(
                PacketInfo::user_packet(
                    event.payload(),
                    event.delivery_guarantee(),
                    event.order_guarantee(),
                    event.expires_after(),
                    event.latest_identifier(),
                ),
                None,
                time,
                time,
            ),
            event.context(),
        );
    }

    /// Processes various connection-related tasks: resend dropped packets, send heartbeat packet, etc...
    /// This function gets called very frequently.
    fn update(
        &mut self,
        messenger: &impl ConnectionMessenger<Self::ReceiveEvent>,
        time: Self::Instant,
    ) {
        // resend dropped packets
        for dropped in self.gather_dropped_packets(time) {
            let packets = self.process_outgoing(
                PacketInfo {
                    packet_type: dropped.packet_type,
                    payload: &dropped.payload,
                    // because a delivery guarantee is only sent with reliable packets
                    delivery: DeliveryGuarantee::Reliable,
                    // this is stored with the dropped packet because they could be mixed
                    ordering: dropped.ordering_guarantee,
                    expires_after: dropped.expires_after,
                    latest_identifier: dropped.latest_identifier,
                },
                dropped.item_identifier,
                time,
                dropped.sent,
            );
            send_packets(messenger, &self.remote_address, packets, "dropped packets");
        }

        // send heartbeat packets if required
        if self.is_established() {
            if let Some(heartbeat_interval) = messenger.config().heartbeat_interval {
                let addr = self.remote_address.clone();
                if self.last_sent(time) >= heartbeat_interval {
                    send_packets(
                        messenger,
                        &addr,
                        self.process_outgoing(PacketInfo::heartbeat_packet(&[]), None, time, time),
                        "heatbeat packet",
                    );
                }
            }
        }
    }
}

// Sends multiple outgoing packets.
fn send_packets(
    ctx: &impl ConnectionMessenger<SocketEvent>,
    address: &SockAddr,
    packets: Result<OutgoingPackets<'_>>,
    err_context: &str,
) {
    match packets {
        Ok(packets) => {
            for outgoing in packets {
                if let Err(err) = ctx.send_packet(address, &outgoing.contents()) {
                    error!(
                        "Error occured sending {} (len={}): {:?}",
                        err_context,
                        outgoing.contents().len(),
                        err
                    );
                }
            }
        }
        Err(error) => error!("Error occured processing {}: {:?}", err_context, error),
    }
}
