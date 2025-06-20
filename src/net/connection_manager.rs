use std::{
    self,
    collections::HashMap,
    fmt::Debug,
    io::{IoSlice, Result},
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use crossbeam_channel::{self, unbounded, Receiver, Sender};
use log::error;
use socket2::SockAddr;

use super::connection::MomentInTime;
use crate::{
    config::Config,
    net::{Connection, ConnectionEventAddress, ConnectionMessenger},
};

static DEBUG_PACKET_SEND: AtomicU64 = AtomicU64::new(0);
static DEBUG_PACKET_RECV: AtomicU64 = AtomicU64::new(0);
static DEBUG_PACKET_DELAY: AtomicU64 = AtomicU64::new(0);
static DEBUG_PACKET_LOSS: AtomicU64 = AtomicU64::new(0);

// TODO: maybe we can make a breaking change and use this instead of `ConnectionEventAddress` trait?
// #[derive(Debug)]
// pub struct ConnectionEvent<Event: Debug>(pub SocketAddr, pub Event);

/// A datagram socket is a type of network socket which provides a connectionless point for sending or receiving data packets.
pub trait DatagramSocket: DatagramSocketSender + DatagramSocketReceiver + Debug {
    /// Returns the socket address that this socket was created from.
    fn split(
        self,
    ) -> (
        Box<dyn DatagramSocketSender + Send + Sync>,
        Box<dyn DatagramSocketReceiver + Send + Sync>,
    );
}

/// A datagram socket is a type of network socket which provides a connectionless point for sending or receiving data packets.
pub trait DatagramSocketSender: Debug {
    /// Clones this sender
    fn clone_box(&self) -> Box<dyn DatagramSocketSender + Send + Sync>;

    /// Sends a single packet to the socket.
    fn send_packet(&self, addr: &SockAddr, payload: &[u8]) -> Result<usize>;

    /// Sends a single packet made up of multiple buffers to the socket.
    fn send_packet_vectored(
        &self,
        addr: &SockAddr,
        bufs: &[IoSlice<'_>],
    ) -> std::io::Result<usize>;
}

/// A datagram socket is a type of network socket which provides a connectionless point for sending or receiving data packets.
pub trait DatagramSocketReceiver: Debug {
    /// Receives a single packet from the socket.
    fn receive_packet<'a>(
        &mut self,
        buffer: &'a mut [MaybeUninit<u8>],
    ) -> Result<(&'a [u8], SockAddr)>;

    /// Returns the socket address that this socket was created from.
    fn local_addr(&self) -> Result<SockAddr>;

    /// Returns whether socket operates in blocking or non-blocking mode.
    fn is_blocking_mode(&self) -> bool;

    /// Sets how long we should block polling for socket events.
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> Result<()>;
}

// This will be used by a `Connection`.
#[derive(Debug)]
struct SocketEventSenderAndConfig<ReceiveEvent: Debug> {
    config: Config,
    socket: Box<dyn DatagramSocketSender + Send + Sync>,
    event_sender: Sender<ReceiveEvent>,
}

impl<ReceiveEvent: Debug> Clone for SocketEventSenderAndConfig<ReceiveEvent> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            socket: self.socket.clone_box(),
            event_sender: self.event_sender.clone(),
        }
    }
}

impl<ReceiveEvent: Debug> SocketEventSenderAndConfig<ReceiveEvent> {
    fn new(
        config: Config,
        socket: Box<dyn DatagramSocketSender + Send + Sync>,
        event_sender: Sender<ReceiveEvent>,
    ) -> Self {
        Self {
            config,
            socket,
            event_sender,
        }
    }
}

impl<ReceiveEvent: Debug> ConnectionMessenger<ReceiveEvent>
    for SocketEventSenderAndConfig<ReceiveEvent>
{
    fn config(&self) -> &Config {
        &self.config
    }

    fn send_event(&self, _address: &SockAddr, event: ReceiveEvent) {
        self.event_sender.send(event).expect("Receiver must exists");
    }

    fn send_packet(&self, address: &SockAddr, payload: &[u8]) -> std::io::Result<()> {
        DEBUG_PACKET_SEND.fetch_add(1, Ordering::SeqCst);
        self.socket.send_packet(address, payload)?;
        Ok(())
    }

    fn send_packet_vectored(
        &self,
        address: &SockAddr,
        bufs: &[IoSlice<'_>],
    ) -> std::io::Result<()> {
        DEBUG_PACKET_SEND.fetch_add(1, Ordering::SeqCst);
        self.socket.send_packet_vectored(address, bufs)?;
        Ok(())
    }
}

/// Sets a packet delay that is used for debugging purposes to test the networking code
pub fn debug_set_packet_delay(packet_delay: Duration) {
    DEBUG_PACKET_DELAY.store(packet_delay.as_millis() as u64, Ordering::Relaxed);
}

/// Gets the current set packet delay value.
pub fn debug_get_packet_delay() -> Duration {
    Duration::from_millis(DEBUG_PACKET_DELAY.load(Ordering::Relaxed))
}

/// Sets a packet loss that is used for debugging purposes to test the networking code
/// Value of `packet_loss` should be between 0.0 and 1.0, where 0.0 means no packet loss and 1.0 means all packets are lost.
pub fn debug_set_packet_loss(packet_loss: f32) {
    DEBUG_PACKET_LOSS.store((packet_loss * 100_000.0) as u64, Ordering::Relaxed);
}

/// Gets the current set packet loss value.
pub fn debug_get_packet_loss() -> f32 {
    DEBUG_PACKET_LOSS.load(Ordering::Relaxed) as f32 / 100_000.0
}

/// Gets the number of packets that were received
pub fn debug_get_packet_recv() -> u64 {
    DEBUG_PACKET_RECV.load(Ordering::SeqCst)
}

/// Resets the packet receive counter
pub fn reset_debug_packet_recv() {
    DEBUG_PACKET_RECV.store(0, Ordering::SeqCst);
}

/// Gets the number of packets that were received
pub fn debug_get_packet_send() -> u64 {
    DEBUG_PACKET_SEND.load(Ordering::SeqCst)
}

/// Resets the packet send counter
pub fn reset_debug_packet_send() {
    DEBUG_PACKET_SEND.store(0, Ordering::SeqCst);
}

/// Implements a concept of connections on top of datagram socket.
/// Connection capabilities depends on what is an actual `Connection` type.
/// Connection type also defines a type of sending and receiving events.
#[derive(Debug)]
pub struct ConnectionManager<TConnection: Connection> {
    connections: Arc<Mutex<HashMap<SockAddr, TConnection>>>,
    receive_buffer: Vec<MaybeUninit<u8>>,
    event_receiver: Receiver<TConnection::ReceiveEvent>,
    delayed_packets: Vec<(TConnection::Instant, SockAddr, Vec<u8>)>,
    max_unestablished_connections: usize,
    cur_read_timeout: Option<Duration>,
    auto_reset: bool,
    rx: Box<dyn DatagramSocketReceiver + Send + Sync>,
    tx: ConnectionSender<TConnection>,
}

impl<TConnection: Connection> ConnectionManager<TConnection> {
    /// Creates an instance of `ConnectionManager` by passing a socket and config.
    pub fn new<TSocket: DatagramSocket>(socket: TSocket, config: Config, auto_reset: bool) -> Self {
        let (event_sender, event_receiver) = unbounded();
        let max_unestablished_connections = config.max_unestablished_connections;

        let (tx, rx) = socket.split();

        let connections = Arc::new(Mutex::new(HashMap::new()));
        ConnectionManager {
            receive_buffer: vec![MaybeUninit::uninit(); config.receive_buffer_max_size],
            connections: connections.clone(),
            rx,
            event_receiver: event_receiver.clone(),
            max_unestablished_connections,
            delayed_packets: Vec::new(),
            cur_read_timeout: None,
            auto_reset,
            tx: ConnectionSender {
                connections,
                tx: SocketEventSenderAndConfig::new(config.clone(), tx, event_sender),
                event_receiver,
            },
        }
    }

    /// Returns true if this connection will automatically reset if the connection ID changes
    pub fn auto_reset(&self) -> bool {
        self.auto_reset
    }

    /// Splits the `ConnectionManager` into two parts: the `ConnectionManagerSender` and the `ConnectionManager`.
    pub fn split(self) -> (ConnectionSender<TConnection>, Self) {
        (self.tx.clone(), self)
    }

    /// Processes any inbound packets and events.
    pub fn manual_poll_inbound(&mut self, now: TConnection::Instant) -> std::io::Result<()> {
        let mut unestablished_connections = self.unestablished_connection_count();
        let messenger = &mut self.tx.tx;

        // function used to process packets
        let mut process_packet = |payload: &[u8], address: SockAddr| {
            DEBUG_PACKET_RECV.fetch_add(1, Ordering::SeqCst);

            let mut connections = self.connections.lock().unwrap();
            if let Some(conn) = connections.get_mut(&address) {
                let was_est = conn.is_established();
                conn.process_packet(messenger, payload, now, self.auto_reset);
                if !was_est && conn.is_established() {
                    unestablished_connections = unestablished_connections.saturating_sub(1);
                }
            } else {
                let mut conn = TConnection::create_connection(messenger, address.clone(), now);
                conn.process_packet(messenger, payload, now, self.auto_reset);

                // We only allow a maximum amount number of unestablished connections to bet created
                // from inbound packets to prevent packet flooding from allocating unbounded memory.
                if unestablished_connections < self.max_unestablished_connections as usize {
                    connections.insert(address, conn);
                    unestablished_connections += 1;
                }
            }
        };

        let packet_loss = DEBUG_PACKET_LOSS.load(Ordering::Relaxed);
        let packet_delay = DEBUG_PACKET_DELAY.load(Ordering::Relaxed);

        // Determine how long we should block for
        let mut loop_duration = Duration::from_millis(10);
        if packet_delay > 0 {
            let packet_delay =
                Duration::from_millis(DEBUG_PACKET_DELAY.load(Ordering::Relaxed));
            loop_duration = self
                .delayed_packets
                .first()
                .map(|p| {
                    let elapsed = now.duration_since(p.0);
                    if elapsed >= packet_delay {
                        Duration::from_millis(1)
                    } else {
                        packet_delay - elapsed
                    }
                })
                .unwrap_or(loop_duration);
        }

        // first we pull all newly arrived packets and handle them
        let started = now;
        let mut n_packets = 0;
        let mut n_loops = 0;
        while TConnection::Instant::now().duration_since(started) <= loop_duration || n_loops < 100 {
            n_loops += 1;

            // Update the read timeout to match
            let read_timeout = loop_duration.saturating_sub(TConnection::Instant::now().duration_since(started));
            let read_timeout = Some(read_timeout);
            if self.cur_read_timeout != read_timeout {
                self.cur_read_timeout = read_timeout;
                self.rx.set_read_timeout(read_timeout).ok();
            }

            // now execute the receive packet operation with the right delay
            let rcv = self.rx.receive_packet(self.receive_buffer.as_mut());
            n_packets += 1;
            match rcv {
                Ok((payload, address)) => {
                    if packet_loss > 0 {
                        let packet_loss = packet_loss as f32 / 100_000.0;
                        if rand::random::<f32>() < packet_loss {
                            continue;
                        }
                    }

                    if packet_delay > 0 {
                        self.delayed_packets
                            .push((now, address.clone(), payload.to_vec()));
                        continue;
                    }

                    process_packet(payload, address);
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                    // this is triggered whenever a packet is sent using the same socket
                    // as it will unblock the blocking action of receiving data
                    continue;
                }
                Err(e) if e.raw_os_error() == Some(10004) => {
                    return Err(e);
                }
                Err(e) if e.raw_os_error() == Some(10093) => {
                    return Err(e);
                }
                Err(e) => {
                    error!("Encountered an error receiving data: {:?}", e);
                    break;
                }
            }
            // prevent from blocking, break after receiving first packet
            if self.rx.is_blocking_mode() {
                break;
            }

            // If we have done a decent number of packets then stop the loop
            // so the delayed packets can also be processed
            if n_packets > 200 {
                break;
            }
        }

        // process any delayed packets
        if !self.delayed_packets.is_empty() {
            let packet_delay = Duration::from_millis(DEBUG_PACKET_DELAY.load(Ordering::Relaxed));
            for (when, address, payload) in self.delayed_packets.iter() {
                let elapsed = now.duration_since(*when);
                if elapsed >= packet_delay {
                    process_packet(payload, address.clone());
                }
            }
            self.delayed_packets.retain(|(when, _, _)| {
                let elapsed = now.duration_since(*when);
                elapsed < packet_delay
            });
        }

        Ok(())
    }

    /// Processes connection specific logic for active connections.
    /// Removes dropped connections from active connections list.
    pub fn manual_poll_update(&mut self, time: TConnection::Instant) {
        self.tx.manual_poll_update(time);
    }

    /// Processes any inbound/outbound packets and events.
    /// Processes connection specific logic for active connections.
    /// Removes dropped connections from active connections list.
    pub fn manual_poll(&mut self, time: TConnection::Instant) -> std::io::Result<()> {
        self.manual_poll_inbound(time)?;
        self.manual_poll_update(time);
        Ok(())
    }

    /// Returns a handle to the event sender which provides a thread-safe way to enqueue user events
    /// to be processed. This should be used when the socket is busy running its polling loop in a
    /// separate thread.
    pub fn event_sender(&self) -> ConnectionSender<TConnection> {
        self.tx.clone()
    }

    /// Returns a handle to the event receiver which provides a thread-safe way to retrieve events
    /// from the connections. This should be used when the socket is busy running its polling loop in
    /// a separate thread.
    pub fn event_receiver(&self) -> &Receiver<TConnection::ReceiveEvent> {
        &self.event_receiver
    }

    /// Returns socket reference.
    pub fn socket_rx(&self) -> &(dyn DatagramSocketReceiver) {
        self.rx.deref()
    }

    /// Returns socket reference.
    pub fn socket_tx(&self) -> &(dyn DatagramSocketSender) {
        self.tx.socket()
    }

    fn unestablished_connection_count(&self) -> usize {
        self.connections
            .lock()
            .unwrap()
            .iter()
            .filter(|c| !c.1.is_established())
            .count()
    }

    /// Returns the number of packets currently in flight.
    pub fn packets_in_flight(&self) -> usize {
        self.delayed_packets.len()
    }

    /// Returns socket mutable reference.
    #[allow(dead_code)]
    pub fn socket_rx_mut(&mut self) -> &mut (dyn DatagramSocketReceiver) {
        self.rx.deref_mut()
    }

    /// Returns socket mutable reference.
    #[allow(dead_code)]
    pub fn socket_tx_mut(&mut self) -> &mut (dyn DatagramSocketSender) {
        self.tx.socket_mut()
    }

    /// Returns a number of active connections.
    #[cfg(test)]
    pub fn connections_count(&self) -> usize {
        self.connections.lock().unwrap().len()
    }
}

/// Implements a concept of connections on top of datagram socket.
/// Connection capabilities depends on what is an actual `Connection` type.
/// Connection type also defines a type of sending and receiving events.
#[derive(Debug)]
pub struct ConnectionSender<TConnection: Connection> {
    tx: SocketEventSenderAndConfig<TConnection::ReceiveEvent>,
    connections: Arc<Mutex<HashMap<SockAddr, TConnection>>>,
    event_receiver: Receiver<TConnection::ReceiveEvent>,
}

impl<TConnection: Connection> Clone for ConnectionSender<TConnection> {
    fn clone(&self) -> Self {
        Self {
            connections: self.connections.clone(),
            tx: self.tx.clone(),
            event_receiver: self.event_receiver.clone(),
        }
    }
}

impl<TConnection: Connection> ConnectionSender<TConnection> {
    /// Sends a single packet to the socket.
    pub fn send_only(&self, event: TConnection::SendEvent) -> Result<()> {
        let now = TConnection::Instant::now();
        self.send_only_with_now(event, now)
    }

    /// Sends a single packet to the socket.
    pub fn send_only_with_now(
        &self,
        event: TConnection::SendEvent,
        now: TConnection::Instant,
    ) -> Result<()> {
        let messenger = &self.tx;

        {
            // get or create connection
            let mut connections = self.connections.lock().unwrap();
            let conn = connections
                .entry(event.address())
                .or_insert_with(|| TConnection::create_connection(messenger, event.address(), now));
            conn.process_event(messenger, event, now);
        }
        Ok(())
    }

    /// Sends a single packet to the socket.
    pub fn send_and_poll(&self, event: TConnection::SendEvent) -> Result<()> {
        let now = TConnection::Instant::now();
        self.send_only_with_now(event, now)?;
        self.manual_poll_update(now);
        Ok(())
    }

    /// Processes connection specific logic for active connections.
    /// Removes dropped connections from active connections list.
    pub fn manual_poll_update(&self, time: TConnection::Instant) {
        let messenger = &self.tx;

        // update all connections
        for conn in self.connections.lock().unwrap().values_mut() {
            conn.update(messenger, time);
        }

        // iterate through all connections and remove those that should be dropped
        self.connections
            .lock()
            .unwrap()
            .retain(|_, conn| !conn.should_drop(messenger, time));
    }

    /// Returns a handle to the event receiver which provides a thread-safe way to retrieve events
    /// from the connections. This should be used when the socket is busy running its polling loop in
    /// a separate thread.
    pub fn event_receiver(&self) -> &Receiver<TConnection::ReceiveEvent> {
        &self.event_receiver
    }

    /// Returns socket reference.
    pub fn socket(&self) -> &(dyn DatagramSocketSender) {
        self.tx.socket.deref()
    }

    /// Returns socket mutable reference.
    #[allow(dead_code)]
    pub fn socket_mut(&mut self) -> &mut (dyn DatagramSocketSender) {
        self.tx.socket.deref_mut()
    }

    /// Returns a number of active connections.
    #[cfg(test)]
    pub fn connections_count(&self) -> usize {
        self.connections.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use coarsetime::Instant;
    use socket2::SockAddr;
    use std::{
        collections::HashSet,
        net::{SocketAddr, SocketAddrV4},
        time::Duration,
    };

    use crate::{debug_get_packet_send, net::connection_manager::{debug_get_packet_recv, reset_debug_packet_recv}, reset_debug_packet_send, test_utils::*};
    use crate::{Config, Packet, SocketEvent};

    /// The socket address of where the server is located.
    const SERVER_ADDR: &str = "127.0.0.1:10001";
    // The client address from where the data is sent.
    const CLIENT_ADDR: &str = "127.0.0.1:10002";

    fn client_address() -> SockAddr {
        let ret: SocketAddr = CLIENT_ADDR.parse().unwrap();
        ret.into()
    }

    fn client_address_n(n: u16) -> SockAddr {
        SocketAddr::V4(SocketAddrV4::new("127.0.0.1".parse().unwrap(), 10002 + n)).into()
    }

    fn server_address() -> SockAddr {
        let ret: SocketAddr = SERVER_ADDR.parse().unwrap();
        ret.into()
    }

    fn create_server_client_network() -> (FakeSocket, FakeSocket, NetworkEmulator) {
        let network = NetworkEmulator::default();
        let server = FakeSocket::bind(&network, server_address(), Config::default()).unwrap();
        let client = FakeSocket::bind(&network, client_address(), Config::default()).unwrap();
        (server, client, network)
    }

    fn create_server_client(config: Config) -> (FakeSocket, FakeSocket) {
        let network = NetworkEmulator::default();
        let server = FakeSocket::bind(&network, server_address(), config.clone()).unwrap();
        let client = FakeSocket::bind(&network, client_address(), config).unwrap();
        (server, client)
    }

    #[test]
    fn using_sender_and_receiver() {
        let (mut server, mut client, _) = create_server_client_network();

        let sender = client.get_packet_sender();
        let receiver = server.get_event_receiver();

        sender
            .send_and_poll(Packet::reliable_unordered(
                server_address(),
                b"Hello world!".to_vec(),
                "user packet",
            ))
            .unwrap();

        let time = Instant::now();
        client.manual_poll(time);
        server.manual_poll(time);

        if let SocketEvent::Packet(packet) = receiver.recv().unwrap() {
            assert_eq![b"Hello world!", packet.payload()];
        } else {
            panic!["Did not receive a packet when it should"];
        }
    }

    #[test]
    fn initial_packet_is_resent() {
        let (mut server, mut client, network) = create_server_client_network();
        let time = Instant::now();

        // send a packet that the server ignores/drops
        client
            .send(Packet::reliable_unordered(
                server_address(),
                b"Do not arrive".to_vec(),
                "user packet",
            ))
            .unwrap();
        client.manual_poll(time);

        // drop the inbound packet, this simulates a network error
        network.clear_packets(server_address());

        // send a packet that the server receives
        for id in 0..u8::max_value() {
            client
                .send(Packet::reliable_unordered(
                    server_address(),
                    vec![id],
                    "user packet",
                ))
                .unwrap();

            server
                .send(Packet::reliable_unordered(
                    client_address(),
                    vec![id],
                    "user packet",
                ))
                .unwrap();

            client.manual_poll(time);
            server.manual_poll(time);

            while let Some(SocketEvent::Packet(pkt)) = server.recv() {
                if pkt.payload() == b"Do not arrive" {
                    return;
                }
            }
            while client.recv().is_some() {}
        }

        panic!["Did not receive the ignored packet"];
    }

    #[test]
    fn receiving_does_not_allow_denial_of_service() {
        let time = Instant::now();
        let network = NetworkEmulator::default();
        let mut server = FakeSocket::bind(
            &network,
            server_address(),
            Config {
                max_unestablished_connections: 2,
                ..Default::default()
            },
        )
        .unwrap();
        // send a bunch of packets to a server
        for i in 0..3 {
            let mut client =
                FakeSocket::bind(&network, client_address_n(i), Config::default()).unwrap();

            client
                .send(Packet::unreliable(
                    server_address(),
                    vec![1, 2, 3, 4, 5, 6, 7, 8, 9],
                    "user packet",
                ))
                .unwrap();

            client.manual_poll(time);
        }

        server.manual_poll(time);

        for _ in 0..3 {
            assert![server.recv().is_some()];
        }
        assert![server.recv().is_none()];

        // the server shall not have at most the configured `max_unestablished_connections` in
        // its connection table even though it packets from 3 clients
        assert_eq![2, server.connection_count()];

        server
            .send(Packet::unreliable(client_address(), vec![1], "user packet"))
            .unwrap();

        server.manual_poll(time);

        // the server only adds to its table after having sent explicitly
        assert_eq![2, server.connection_count()];
    }

    #[test]
    fn initial_sequenced_is_resent() {
        let (mut server, mut client, network) = create_server_client_network();
        let time = Instant::now();

        // send a packet that the server ignores/drops
        client
            .send(Packet::reliable_sequenced(
                server_address(),
                b"Do not arrive".to_vec(),
                None,
                "user packet",
            ))
            .unwrap();
        client.manual_poll(time);

        // drop the inbound packet, this simulates a network error
        network.clear_packets(server_address());

        // send a packet that the server receives
        for id in 0..36 {
            client
                .send(Packet::reliable_sequenced(
                    server_address(),
                    vec![id],
                    None,
                    "user packet",
                ))
                .unwrap();

            server
                .send(Packet::reliable_sequenced(
                    client_address(),
                    vec![id],
                    None,
                    "user packet",
                ))
                .unwrap();

            client.manual_poll(time);
            server.manual_poll(time);

            while let Some(SocketEvent::Packet(pkt)) = server.recv() {
                if pkt.payload() == b"Do not arrive" {
                    panic!["Sequenced packet arrived while it should not"];
                }
            }
            while client.recv().is_some() {}
        }
    }

    #[test]
    fn initial_ordered_is_resent() {
        let (mut server, mut client, network) = create_server_client_network();
        let time = Instant::now();

        // send a packet that the server ignores/drops
        client
            .send(Packet::reliable_ordered(
                server_address(),
                b"Do not arrive".to_vec(),
                None,
                "user packet",
            ))
            .unwrap();
        client.manual_poll(time);

        // drop the inbound packet, this simulates a network error
        network.clear_packets(server_address());

        // send a packet that the server receives
        for id in 0..35 {
            client
                .send(Packet::reliable_ordered(
                    server_address(),
                    vec![id],
                    None,
                    "user packet",
                ))
                .unwrap();

            server
                .send(Packet::reliable_ordered(
                    client_address(),
                    vec![id],
                    None,
                    "user packet",
                ))
                .unwrap();

            client.manual_poll(time);
            server.manual_poll(time);

            while let Some(SocketEvent::Packet(pkt)) = server.recv() {
                if pkt.payload() == b"Do not arrive" {
                    return;
                }
            }
            while client.recv().is_some() {}
        }

        panic!["Did not receive the ignored packet"];
    }

    #[test]
    fn do_not_duplicate_sequenced_packets_when_received() {
        let (mut server, mut client, _) = create_server_client_network();
        let time = Instant::now();

        for id in 0..100 {
            client
                .send(Packet::reliable_sequenced(
                    server_address(),
                    vec![id],
                    None,
                    "user packet",
                ))
                .unwrap();
            client.manual_poll(time);
            server.manual_poll(time);
        }

        let mut seen = HashSet::new();

        while let Some(message) = server.recv() {
            match message {
                SocketEvent::Connect(_) => {}
                SocketEvent::Packet(packet) => {
                    let byte = packet.payload()[0];
                    assert![!seen.contains(&byte)];
                    seen.insert(byte);
                }
                SocketEvent::Timeout(_) | SocketEvent::Overload(_) | SocketEvent::Disconnect(_) => {
                    panic!["This should not happen, as we've not advanced time"];
                }
            }
        }

        assert_eq![100, seen.len()];
    }

    #[test]
    fn more_than_65536_sequenced_packets() {
        reset_debug_packet_recv();
        reset_debug_packet_send();
        
        let (mut server, mut client, _) = create_server_client_network();
        // acknowledge the client
        server
            .send(Packet::unreliable(client_address(), vec![0], "user packet"))
            .unwrap();

        let time = Instant::now();

        for id in 0..65536 + 100 {
            client
                .send(Packet::unreliable_sequenced(
                    server_address(),
                    id.to_string().as_bytes().to_vec(),
                    None,
                    "user packet",
                ))
                .unwrap();
            client.manual_poll(time);
            server.manual_poll(time);
        }

        let mut connect = 0;
        let mut cnt = 0;
        while let Some(message) = server.recv() {
            match message {
                SocketEvent::Connect(_) => {
                    connect += 1;
                }
                SocketEvent::Packet(_) => {
                    cnt += 1;
                }
                SocketEvent::Timeout(_) | SocketEvent::Overload(_) | SocketEvent::Disconnect(_) => {
                    panic!["This should not happen, as we've not advanced time"];
                }
            }
        }

        assert!(debug_get_packet_send() >= 65536 + 100 + 1);
        assert!(debug_get_packet_recv() >= 65536 + 100 + 1);
        assert_eq![1, connect];
        assert_eq![65536 + 100, cnt];
    }

    #[test]
    fn sequenced_packets_pathological_case() {
        let config = Config {
            max_packets_in_flight: 100,
            ..Default::default()
        };
        let (_, mut client) = create_server_client(config);

        let time = Instant::now();

        for id in 0..101 {
            client
                .send(Packet::reliable_sequenced(
                    server_address(),
                    id.to_string().as_bytes().to_vec(),
                    None,
                    "user packet",
                ))
                .unwrap();
            client.manual_poll(time);

            while let Some(event) = client.recv() {
                match event {
                    SocketEvent::Overload(remote_addr) => {
                        assert_eq![100, id];
                        assert_eq![remote_addr, server_address()];
                        return;
                    }
                    _ => {
                        panic!["No other event possible"];
                    }
                }
            }
        }

        panic!["Should have received a timeout event"];
    }

    #[test]
    fn manual_polling_socket() {
        let (mut server, mut client, _) = create_server_client_network();
        for _ in 0..3 {
            client
                .send(Packet::unreliable(
                    server_address(),
                    vec![1, 2, 3, 4, 5, 6, 7, 8, 9],
                    "user packet",
                ))
                .unwrap();
        }

        let time = Instant::now();

        client.manual_poll(time);
        server.manual_poll(time);

        assert!(server.recv().is_some());
        assert!(server.recv().is_some());
        assert!(server.recv().is_some());
    }

    #[test]
    fn can_send_and_receive() {
        let (mut server, mut client, _) = create_server_client_network();
        for _ in 0..3 {
            client
                .send(Packet::unreliable(
                    server_address(),
                    vec![1, 2, 3, 4, 5, 6, 7, 8, 9],
                    "user packet",
                ))
                .unwrap();
        }

        let now = Instant::now();
        client.manual_poll(now);
        server.manual_poll(now);

        assert!(server.recv().is_some());
        assert!(server.recv().is_some());
        assert!(server.recv().is_some());
    }

    #[test]
    fn connect_event_occurs() {
        let (mut server, mut client, _) = create_server_client_network();

        client
            .send(Packet::unreliable(
                server_address(),
                vec![0, 1, 2],
                "user packet",
            ))
            .unwrap();

        server
            .send(Packet::unreliable(
                client_address(),
                vec![2, 1, 0],
                "user packet",
            ))
            .unwrap();

        let now = Instant::now();
        client.manual_poll(now);
        server.manual_poll(now);

        assert_eq!(
            server.recv().unwrap(),
            SocketEvent::Connect(client_address())
        );
        assert!(matches!(server.recv().unwrap(), SocketEvent::Packet(_)));
    }

    #[test]
    fn disconnect_event_occurs() {
        let config = Config {
            idle_connection_timeout: Duration::from_millis(1),
            ..Default::default()
        };
        let (mut server, mut client) = create_server_client(config.clone());

        client
            .send(Packet::unreliable(
                server_address(),
                vec![0, 1, 2],
                "user packet",
            ))
            .unwrap();

        let now = Instant::now();
        client.manual_poll(now);
        server.manual_poll(now);

        assert_eq!(
            server.recv().unwrap(),
            SocketEvent::Packet(Packet::unreliable(
                client_address(),
                vec![0, 1, 2],
                "received packet"
            ))
        );

        // acknowledge the client
        server
            .send(Packet::unreliable(client_address(), vec![], "user packet"))
            .unwrap();

        server.manual_poll(now);
        client.manual_poll(now);

        assert_eq!(
            server.recv().unwrap(),
            SocketEvent::Connect(client_address())
        );

        // make sure the connection was successful on the client side
        assert_eq!(
            client.recv().unwrap(),
            SocketEvent::Connect(server_address())
        );
        assert_eq!(
            client.recv().unwrap(),
            SocketEvent::Packet(Packet::unreliable(
                server_address(),
                vec![],
                "received packet"
            ))
        );

        // give just enough time for no timeout events to occur (yet)
        server.manual_poll(
            now + coarsetime::Duration::from(config.idle_connection_timeout)
                - coarsetime::Duration::from_millis(50),
        );
        client.manual_poll(
            now + coarsetime::Duration::from(config.idle_connection_timeout)
                - coarsetime::Duration::from_millis(50),
        );

        assert_eq!(server.recv(), None);
        assert_eq!(client.recv(), None);

        // give enough time for timeouts to be detected
        server.manual_poll(
            now + coarsetime::Duration::from(config.idle_connection_timeout)
                + coarsetime::Duration::from_millis(50),
        );
        client.manual_poll(
            now + coarsetime::Duration::from(config.idle_connection_timeout)
                + coarsetime::Duration::from_millis(50),
        );

        assert_eq!(
            server.recv().unwrap(),
            SocketEvent::Timeout(client_address())
        );
        assert_eq!(
            server.recv().unwrap(),
            SocketEvent::Disconnect(client_address())
        );
        assert_eq!(
            client.recv().unwrap(),
            SocketEvent::Timeout(server_address())
        );
        assert_eq!(
            client.recv().unwrap(),
            SocketEvent::Disconnect(server_address())
        );
    }

    #[test]
    fn heartbeats_work() {
        let config = Config {
            idle_connection_timeout: Duration::from_millis(10),
            heartbeat_interval: Some(Duration::from_millis(4)),
            ..Default::default()
        };
        let (mut server, mut client) = create_server_client(config.clone());
        // initiate a connection
        client
            .send(Packet::unreliable(
                server_address(),
                vec![0, 1, 2],
                "user packet",
            ))
            .unwrap();

        let now = Instant::now();
        client.manual_poll(now);
        server.manual_poll(now);

        assert_eq!(
            server.recv().unwrap(),
            SocketEvent::Packet(Packet::unreliable(
                client_address(),
                vec![0, 1, 2],
                "received packet"
            ))
        );

        // acknowledge the client
        // this way, the server also knows about the connection and sends heartbeats
        server
            .send(Packet::unreliable(client_address(), vec![], "user packet"))
            .unwrap();

        server.manual_poll(now);
        client.manual_poll(now);

        // make sure the connection was successful on the server side
        assert_eq!(
            server.recv().unwrap(),
            SocketEvent::Connect(client_address())
        );

        // make sure the connection was successful on the server side
        assert_eq!(
            client.recv().unwrap(),
            SocketEvent::Connect(server_address())
        );

        // make sure the connection was successful on the client side
        assert_eq!(
            client.recv().unwrap(),
            SocketEvent::Packet(Packet::unreliable(
                server_address(),
                vec![],
                "received packet"
            ))
        );

        // give time to send heartbeats
        client.manual_poll(now + coarsetime::Duration::from(config.heartbeat_interval.unwrap()));
        server.manual_poll(now + coarsetime::Duration::from(config.heartbeat_interval.unwrap()));

        // give time for timeouts to occur if no heartbeats were sent
        client.manual_poll(now + coarsetime::Duration::from(config.idle_connection_timeout));
        server.manual_poll(now + coarsetime::Duration::from(config.idle_connection_timeout));

        // assert that no disconnection events occurred
        assert_eq!(client.recv(), None);
        assert_eq!(server.recv(), None);
    }

    #[test]
    fn multiple_sends_should_start_sending_dropped() {
        let (mut server, mut client, _) = create_server_client_network();

        let now = Instant::now();

        // send enough packets to ensure that we must have dropped packets.
        for i in 0..35 {
            client
                .send(Packet::unreliable(server_address(), vec![i], "user packet"))
                .unwrap();
            client.manual_poll(now);
        }

        let mut events = Vec::new();

        loop {
            server.manual_poll(now);
            if let Some(event) = server.recv() {
                events.push(event);
            } else {
                break;
            }
        }

        // ensure that we get the correct number of events to the server.
        // 0 connect events plus the 35 messages
        assert_eq!(events.len(), 35);

        // finally the server decides to send us a message back. This necessarily will include
        // the ack information for 33 of the sent 35 packets.
        server
            .send(Packet::unreliable(client_address(), vec![0], "user packet"))
            .unwrap();
        server.manual_poll(now);

        // make sure the connection was successful on the server side
        assert_eq!(
            server.recv().unwrap(),
            SocketEvent::Connect(client_address())
        );

        // loop to ensure that the client gets the server message before moving on
        loop {
            client.manual_poll(now);
            if client.recv().is_some() {
                break;
            }
        }

        // this next sent message should end up sending the 2 unacked messages plus the new messages
        // with payload 35
        events.clear();
        client
            .send(Packet::unreliable(
                server_address(),
                vec![35],
                "user packet",
            ))
            .unwrap();
        client.manual_poll(now);

        loop {
            server.manual_poll(now);
            if let Some(event) = server.recv() {
                events.push(event);
                break;
            }
        }

        let sent_events: Vec<u8> = events
            .iter()
            .flat_map(|e| match e {
                SocketEvent::Packet(p) => Some(p.payload()[0]),
                _ => None,
            })
            .collect();
        assert_eq!(sent_events, vec![35]);
    }

    #[test]
    fn fragmented_ordered_gets_acked() {
        let config = Config {
            fragment_size: 10,
            ..Default::default()
        };
        let (mut server, mut client) = create_server_client(config);

        let time = Instant::now();
        let dummy = vec![0];

        // ---

        client
            .send(Packet::unreliable(
                server_address(),
                dummy.clone(),
                "user packet",
            ))
            .unwrap();
        client.manual_poll(time);
        server
            .send(Packet::unreliable(
                client_address(),
                dummy.clone(),
                "received packet",
            ))
            .unwrap();
        server.manual_poll(time);

        // ---

        let exceeds = b"Fragmented string".to_vec();
        client
            .send(Packet::reliable_ordered(
                server_address(),
                exceeds,
                None,
                "user packet",
            ))
            .unwrap();
        client.manual_poll(time);

        server.manual_poll(time);
        server.manual_poll(time);
        server
            .send(Packet::reliable_ordered(
                client_address(),
                dummy.clone(),
                None,
                "user packet",
            ))
            .unwrap();

        client
            .send(Packet::unreliable(
                server_address(),
                dummy.clone(),
                "user packet",
            ))
            .unwrap();
        client.manual_poll(time);
        server.manual_poll(time);

        for _ in 0..4 {
            assert![server.recv().is_some()];
        }
        assert![server.recv().is_none()];

        for _ in 0..34 {
            client
                .send(Packet::reliable_ordered(
                    server_address(),
                    dummy.clone(),
                    None,
                    "user packet",
                ))
                .unwrap();
            client.manual_poll(time);
            server
                .send(Packet::reliable_ordered(
                    client_address(),
                    dummy.clone(),
                    None,
                    "user packet",
                ))
                .unwrap();
            server.manual_poll(time);
            assert![client.recv().is_some()];
            // If the last iteration returns `None` here, it indicates we just received a re-sent
            // fragment, because `manual_poll` only processes a single incoming UDP packet per
            // `manual_poll` if and only if the socket is in blocking mode.
            //
            // If that functionality is changed, we will receive something unexpected here
            match server.recv() {
                Some(SocketEvent::Packet(pkt)) => {
                    assert_eq![dummy, pkt.payload()];
                }
                _ => {
                    panic!["Did not receive expected dummy packet"];
                }
            }
        }
    }

    /*/
    #[quickcheck_macros::quickcheck]
    fn do_not_panic_on_arbitrary_packets(bytes: Vec<u8>) {
        use crate::net::DatagramSocketSender;
        let network = NetworkEmulator::default();
        let mut server = FakeSocket::bind(&network, server_address(), Config::default()).unwrap();
        let client_socket = network.new_socket(client_address()).unwrap();

        client_socket
            .send_packet(&server_address(), &bytes)
            .unwrap();

        let time = Instant::now();
        server.manual_poll(time);
    }
    */
}
