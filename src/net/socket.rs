use std::{
    self,
    io::IoSlice,
    mem::MaybeUninit,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs},
    thread::{sleep, yield_now},
    time::Duration,
};

use crossbeam_channel::{self, Receiver, TryRecvError};
use socket2::SockAddr;

use crate::{
    config::Config,
    error::Result,
    net::{
        events::SocketEvent, ConnectionManager, DatagramSocket, LinkConditioner, VirtualConnection,
    },
    packet::Packet,
};

use super::{
    connection::MomentInTime, connection_manager::ConnectionSender, DatagramSocketReceiver,
    DatagramSocketSender,
};

fn create_socket(listen_addr: socket2::SockAddr) -> std::io::Result<socket2::Socket> {
    let socket = socket2::Socket::new(
        match listen_addr.is_ipv4() {
            true => socket2::Domain::IPV4,
            false => socket2::Domain::IPV6,
        },
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    socket.set_nodelay(true).ok();
    socket.set_reuse_address(true).unwrap();
    socket.bind(&listen_addr.clone()).unwrap();
    Ok(socket)
}

#[derive(Debug)]
struct SocketWithConditionerTx {
    socket_tx: socket2::Socket,
    link_conditioner: Option<LinkConditioner>,
}

#[derive(Debug)]
struct SocketWithConditionerRx {
    is_blocking_mode: bool,
    socket_rx: socket2::Socket,
}

// Wraps `LinkConditioner` and `UdpSocket` together. LinkConditioner is enabled when building with a "tester" feature.
#[derive(Debug)]
struct SocketWithConditioner {
    tx: SocketWithConditionerTx,
    rx: SocketWithConditionerRx,
}

impl SocketWithConditioner {
    pub fn new(socket: socket2::Socket, is_blocking_mode: bool) -> Result<Self> {
        let socket_tx = create_socket(socket.local_addr()?)?;
        let socket_rx = socket;
        Ok(SocketWithConditioner {
            rx: SocketWithConditionerRx {
                is_blocking_mode,
                socket_rx,
            },
            tx: SocketWithConditionerTx {
                socket_tx,
                link_conditioner: None,
            },
        })
    }

    #[cfg(feature = "tester")]
    pub fn set_link_conditioner(&mut self, link_conditioner: Option<LinkConditioner>) {
        self.link_conditioner = link_conditioner;
    }
}

impl DatagramSocket for SocketWithConditioner {
    fn split(
        self,
    ) -> (
        Box<dyn DatagramSocketSender + Send + Sync>,
        Box<dyn DatagramSocketReceiver + Send + Sync>,
    ) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}

impl DatagramSocketSender for SocketWithConditioner {
    fn clone_box(&self) -> Box<dyn DatagramSocketSender + Send + Sync> {
        Box::new(SocketWithConditionerTx {
            socket_tx: self.tx.socket_tx.try_clone().unwrap(),
            link_conditioner: self.tx.link_conditioner.clone(),
        })
    }

    fn send_packet(&self, addr: &SockAddr, payload: &[u8]) -> std::io::Result<usize> {
        self.tx.send_packet(addr, payload)
    }

    fn send_packet_vectored(
        &mut self,
        addr: &SockAddr,
        bufs: &[IoSlice<'_>],
    ) -> std::io::Result<usize> {
        self.tx.send_packet_vectored(addr, bufs)
    }
}

impl DatagramSocketReceiver for SocketWithConditioner {
    fn receive_packet<'a>(
        &mut self,
        buffer: &'a mut [MaybeUninit<u8>],
    ) -> std::io::Result<(&'a [u8], SockAddr)> {
        self.rx.receive_packet(buffer)
    }

    fn local_addr(&self) -> std::io::Result<SockAddr> {
        self.rx.local_addr()
    }

    fn is_blocking_mode(&self) -> bool {
        self.rx.is_blocking_mode
    }
}

/// Provides a `DatagramSocket` implementation for `SocketWithConditioner`
impl DatagramSocketSender for SocketWithConditionerTx {
    // Clones the `SocketWithConditionerTx` struct.
    fn clone_box(&self) -> Box<dyn DatagramSocketSender + Send + Sync> {
        Box::new(SocketWithConditionerTx {
            socket_tx: self.socket_tx.try_clone().unwrap(),
            link_conditioner: self.link_conditioner.clone(),
        })
    }

    // Determinate whether packet will be sent or not based on `LinkConditioner` if enabled.
    fn send_packet(&self, addr: &SockAddr, payload: &[u8]) -> std::io::Result<usize> {
        if cfg!(feature = "tester") {
            if let Some(ref link) = &self.link_conditioner {
                if !link.should_send() {
                    return Ok(0);
                }
            }
        }
        self.socket_tx.send_to(payload, addr)
    }

    // Determinate whether packet will be sent or not based on `LinkConditioner` if enabled.
    fn send_packet_vectored(
        &mut self,
        addr: &SockAddr,
        bufs: &[IoSlice<'_>],
    ) -> std::io::Result<usize> {
        if cfg!(feature = "tester") {
            if let Some(ref mut link) = &mut self.link_conditioner {
                if !link.should_send() {
                    return Ok(0);
                }
            }
        }
        self.socket_tx.send_to_vectored(bufs, addr)
    }
}

impl DatagramSocketReceiver for SocketWithConditionerRx {
    /// Receives a single packet from UDP socket.
    fn receive_packet<'a>(
        &mut self,
        buffer: &'a mut [MaybeUninit<u8>],
    ) -> std::io::Result<(&'a [u8], SockAddr)> {
        self.socket_rx
            .recv_from(buffer)
            .map(move |(recv_len, address)| {
                let buffer = unsafe { std::mem::transmute(&buffer[..recv_len]) };
                (buffer, address)
            })
    }

    /// Returns the socket address that this socket was created from.
    fn local_addr(&self) -> std::io::Result<SockAddr> {
        self.socket_rx.local_addr()
    }

    /// Returns whether socket operates in blocking or non-blocking mode.
    fn is_blocking_mode(&self) -> bool {
        self.is_blocking_mode
    }
}

impl DatagramSocketSender for Box<dyn DatagramSocketSender> {
    fn clone_box(&self) -> Box<dyn DatagramSocketSender + Send + Sync> {
        (**self).clone_box()
    }

    fn send_packet(&self, addr: &SockAddr, payload: &[u8]) -> std::io::Result<usize> {
        (**self).send_packet(addr, payload)
    }

    fn send_packet_vectored(
        &mut self,
        addr: &SockAddr,
        bufs: &[IoSlice<'_>],
    ) -> std::io::Result<usize> {
        (**self).send_packet_vectored(addr, bufs)
    }
}

impl DatagramSocketReceiver for Box<dyn DatagramSocketReceiver> {
    fn receive_packet<'a>(
        &mut self,
        buffer: &'a mut [MaybeUninit<u8>],
    ) -> std::io::Result<(&'a [u8], SockAddr)> {
        (**self).receive_packet(buffer)
    }

    fn local_addr(&self) -> std::io::Result<SockAddr> {
        (**self).local_addr()
    }

    fn is_blocking_mode(&self) -> bool {
        (**self).is_blocking_mode()
    }
}

/// A reliable UDP socket implementation with configurable reliability and ordering guarantees.
#[derive(Debug)]
pub struct Socket<T: MomentInTime> {
    handler: ConnectionManager<VirtualConnection<T>>,
}

impl<T: MomentInTime> Socket<T> {
    /// Binds to the socket and then sets up `ActiveConnections` to manage the "connections".
    /// Because UDP connections are not persistent, we can only infer the status of the remote
    /// endpoint by looking to see if they are still sending packets or not
    pub fn bind<A: ToSocketAddrs>(addresses: A) -> Result<Self> {
        Self::bind_with_config(addresses, Config::default())
    }

    /// Binds to any local port on the system, if available
    pub fn bind_any() -> Result<Self> {
        Self::bind_any_with_config(Config::default())
    }

    /// Binds to any local port on the system, if available, with a given config
    pub fn bind_any_with_config(config: Config) -> Result<Self> {
        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        let address = SocketAddrV4::new(loopback, 0);
        let socket = create_socket(address.into())?;
        Self::bind_internal(socket, config)
    }

    /// Binds to the socket and then sets up `ActiveConnections` to manage the "connections".
    /// Because UDP connections are not persistent, we can only infer the status of the remote
    /// endpoint by looking to see if they are still sending packets or not
    ///
    /// This function allows you to configure laminar with the passed configuration.
    pub fn bind_with_config<A: ToSocketAddrs>(addresses: A, config: Config) -> Result<Self> {
        let address: SocketAddr = addresses.to_socket_addrs()?.next().unwrap();
        let socket = create_socket(address.into())?;
        Self::bind_internal(socket, config)
    }

    fn bind_internal(socket: socket2::Socket, config: Config) -> Result<Self> {
        Ok(Socket {
            handler: ConnectionManager::new(
                SocketWithConditioner::new(socket, config.blocking_mode)?,
                config,
            ),
        })
    }

    /// Splits the socket its a sender and receiver half
    pub fn split(self) -> (SocketTx<T>, SocketRx<T>) {
        let (tx, rx) = self.handler.split();
        (SocketTx { handler: tx }, SocketRx { handler: rx })
    }

    /// Returns a handle to the packet sender which provides a thread-safe way to enqueue packets
    /// to be processed. This should be used when the socket is busy running its polling loop in a
    /// separate thread.
    pub fn get_packet_sender(&self) -> ConnectionSender<VirtualConnection<T>> {
        self.handler.event_sender()
    }

    /// Returns a handle to the event receiver which provides a thread-safe way to retrieve events
    /// from the socket. This should be used when the socket is busy running its polling loop in
    /// a separate thread.
    pub fn get_event_receiver(&self) -> Receiver<SocketEvent> {
        self.handler.event_receiver().clone()
    }

    /// Sends a single packet
    pub fn send(&mut self, packet: Packet) -> Result<()> {
        self.handler
            .event_sender()
            .send_only(packet)
            .expect("Receiver must exists.");
        Ok(())
    }

    /// Sends a single packet and polls
    pub fn send_and_poll(&mut self, packet: Packet) -> Result<()> {
        self.handler
            .event_sender()
            .send_and_poll(packet)
            .expect("Receiver must exists.");
        Ok(())
    }

    /// Receives a single packet
    pub fn recv(&mut self) -> Option<SocketEvent> {
        match self.handler.event_receiver().try_recv() {
            Ok(pkt) => Some(pkt),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!["This can never happen"],
        }
    }

    /// Runs the polling loop with the default '1ms' sleep duration. This should run in a spawned thread
    /// since calls to `self.manual_poll` are blocking.
    pub fn start_polling(&mut self) -> std::io::Result<()> {
        self.start_polling_with_duration(Some(Duration::from_millis(1)))?;
        Ok(())
    }

    /// Runs the polling loop with a specified sleep duration. This should run in a spawned thread
    /// since calls to `self.manual_poll` are blocking.
    pub fn start_polling_with_duration(
        &mut self,
        sleep_duration: Option<Duration>,
    ) -> std::io::Result<()> {
        // nothing should break out of this loop!
        loop {
            self.manual_poll(T::now())?;
            match sleep_duration {
                None => yield_now(),
                Some(duration) => sleep(duration),
            };
        }
    }

    /// Processes any inbound/outbound packets and handle idle clients
    pub fn manual_poll_inbound(&mut self, time: T) -> std::io::Result<()> {
        self.handler.manual_poll_inbound(time)?;
        Ok(())
    }

    /// Processes any inbound/outbound packets and handle idle clients
    pub fn manual_poll_update(&mut self, time: T) {
        self.handler.manual_poll_update(time);
    }

    /// Processes any inbound/outbound packets and handle idle clients
    pub fn manual_poll(&mut self, time: T) -> std::io::Result<()> {
        self.handler.manual_poll(time)?;
        Ok(())
    }

    /// Returns the local socket address
    pub fn local_addr(&self) -> Result<SockAddr> {
        Ok(self.handler.socket_rx().local_addr()?)
    }

    /// Sets the link conditioner for this socket. See [LinkConditioner] for further details.
    #[cfg(feature = "tester")]
    pub fn set_link_conditioner(&mut self, link_conditioner: Option<LinkConditioner>) {
        self.handler
            .socket_mut()
            .set_link_conditioner(link_conditioner);
    }
}

/// A reliable UDP socket implementation with configurable reliability and ordering guarantees.
#[derive(Debug)]
pub struct SocketTx<T: MomentInTime> {
    handler: ConnectionSender<VirtualConnection<T>>,
}

impl<T: MomentInTime> SocketTx<T> {
    /// Sends a single packet
    pub fn send(&mut self, packet: Packet) -> Result<()> {
        self.handler.send_and_poll(packet)?;
        Ok(())
    }
}

/// A reliable UDP socket implementation with configurable reliability and ordering guarantees.
#[derive(Debug)]
pub struct SocketRx<T: MomentInTime> {
    handler: ConnectionManager<VirtualConnection<T>>,
}

impl<T: MomentInTime> SocketRx<T> {
    /// Returns a handle to the event receiver which provides a thread-safe way to retrieve events
    /// from the socket. This should be used when the socket is busy running its polling loop in
    /// a separate thread.
    pub fn get_event_receiver(&self) -> Receiver<SocketEvent> {
        self.handler.event_receiver().clone()
    }

    /// Receives a single packet
    pub fn recv(&mut self) -> Option<SocketEvent> {
        match self.handler.event_receiver().try_recv() {
            Ok(pkt) => Some(pkt),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!["This can never happen"],
        }
    }

    /// Runs the polling loop with the default '1ms' sleep duration. This should run in a spawned thread
    /// since calls to `self.manual_poll` are blocking.
    pub fn start_polling(&mut self) -> std::io::Result<()> {
        self.start_polling_with_duration(Some(Duration::from_millis(1)))?;
        Ok(())
    }

    /// Runs the polling loop with a specified sleep duration. This should run in a spawned thread
    /// since calls to `self.manual_poll` are blocking.
    pub fn start_polling_with_duration(
        &mut self,
        sleep_duration: Option<Duration>,
    ) -> std::io::Result<()> {
        // nothing should break out of this loop!
        loop {
            self.manual_poll(T::now())?;
            match sleep_duration {
                None => yield_now(),
                Some(duration) => sleep(duration),
            };
        }
    }

    /// Processes any inbound/outbound packets and handle idle clients
    pub fn manual_poll(&mut self, time: T) -> std::io::Result<()> {
        self.handler.manual_poll(time)?;
        Ok(())
    }

    /// Returns the local socket address
    pub fn local_addr(&self) -> Result<SockAddr> {
        Ok(self.handler.socket_rx().local_addr()?)
    }
}
