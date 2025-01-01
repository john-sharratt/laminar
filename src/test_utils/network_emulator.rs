use std::{
    collections::{hash_map::Entry, HashMap, VecDeque}, io::Result, mem::MaybeUninit, sync::{Arc, Mutex}
};

use socket2::SockAddr;

use crate::{net::LinkConditioner, DatagramSocket, DatagramSocketReceiver, DatagramSocketSender};

/// This type allows to share global state between all sockets, created from the same instance of `NetworkEmulator`.
type GlobalBindings = Arc<Mutex<HashMap<SockAddr, VecDeque<(SockAddr, Vec<u8>)>>>>;

/// Enables to create the emulated socket, that share global state stored by this network emulator.
#[derive(Debug, Default)]
pub struct NetworkEmulator {
    network: GlobalBindings,
}

impl NetworkEmulator {
    /// Creates an emulated socket by binding to an address.
    /// If other socket already was bound to this address, error will be returned instead.
    pub fn new_socket(&self, address: SockAddr) -> Result<EmulatedSocket> {
        match self.network.lock().unwrap().entry(address.clone()) {
            Entry::Occupied(_) => Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "Cannot bind to address",
            )),
            Entry::Vacant(entry) => {
                entry.insert(Default::default());
                Ok(EmulatedSocket {
                    network: self.network.clone(),
                    address,
                    conditioner: Default::default(),
                })
            }
        }
    }

    /// Clear all packets from a socket that is bound to provided address.
    pub fn clear_packets(&self, addr: SockAddr) {
        if let Some(packets) = self.network.lock().unwrap().get_mut(&addr) {
            packets.clear();
        }
    }
}

/// Implementation of a socket, that is created by `NetworkEmulator`.
#[derive(Debug, Clone)]
pub struct EmulatedSocket {
    network: GlobalBindings,
    address: SockAddr,
    conditioner: Option<LinkConditioner>,
}

impl EmulatedSocket {
    pub fn set_link_conditioner(&mut self, conditioner: Option<LinkConditioner>) {
        self.conditioner = conditioner;
    }
}

impl DatagramSocket for EmulatedSocket {
    fn split(self) -> (Box<dyn DatagramSocketSender + Send + Sync>, Box<dyn DatagramSocketReceiver + Send + Sync>) {
        (Box::new(self.clone()), Box::new(self))
    }
}

impl DatagramSocketSender for EmulatedSocket {
    /// Clones this emulated socket
    fn clone_box(&self) -> Box<dyn DatagramSocketSender + Send + Sync> {
        Box::new(self.clone())
    }

    /// Sends a packet to and address if there is a socket bound to it. Otherwise it will simply be ignored.
    fn send_packet(&mut self, addr: &SockAddr, payload: &[u8]) -> Result<usize> {
        let send = if let Some(ref mut conditioner) = self.conditioner {
            conditioner.should_send()
        } else {
            true
        };
        if send {
            if let Some(binded) = self.network.lock().unwrap().get_mut(addr) {
                binded.push_back((self.address.clone(), payload.to_vec()));
            }
            Ok(payload.len())
        } else {
            Ok(0)
        }
    }

    fn send_packet_vectored(&mut self, addr: &SockAddr, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
        
        let send = if let Some(ref mut conditioner) = self.conditioner {
            conditioner.should_send()
        } else {
            true
        };
        if send {
            let payload = bufs.iter().flat_map(|buf| buf.iter().copied()).collect::<Vec<u8>>();
            let payload_len= payload.len();
            if let Some(binded) = self.network.lock().unwrap().get_mut(addr) {
                binded.push_back((self.address.clone(), payload));
            }
            Ok(payload_len)
        } else {
            Ok(0)
        }
    }
}

impl DatagramSocketReceiver for EmulatedSocket {
    /// Receives a packet from this socket.
    fn receive_packet<'a>(&mut self, buffer: &'a mut [MaybeUninit<u8>]) -> Result<(&'a [u8], SockAddr)> {
        if let Some((addr, payload)) = self
            .network
            .lock()
            .unwrap()
            .get_mut(&self.address)
            .unwrap()
            .pop_front()
        {
            let slice = &mut buffer[..payload.len()];
            let slice = unsafe { std::mem::transmute::<&mut [MaybeUninit<u8>], &mut [u8]>(slice) };
            slice.copy_from_slice(payload.as_ref());
            Ok((slice, addr.into()))
        } else {
            Err(std::io::ErrorKind::WouldBlock.into())
        }
    }

    /// Returns the socket address that this socket was created from.
    fn local_addr(&self) -> Result<SockAddr> {
        Ok(self.address.clone())
    }

    fn is_blocking_mode(&self) -> bool {
        false
    }
}
