//! Implements UDP networking for real-time game data sync. This is essentially
//! an attempt to implement GafferOnGames' approach to game world sync.

use std::io;
use std::marker::PhantomData;
use std::mem::size_of;
use std::time::Duration;

use bytemuck::{AnyBitPattern, NoUninit, PodCastError};
pub const PAYLOAD_LEN: usize = 1024;
pub const MSG_LEN: usize = size_of::<Message>();

/// Maximum number of un-acked packets. u32 datatype is used as a bitvec, so the
/// max value this can be is 32. TODO:
///     - make this a parameter to Peer.
pub const MAX_UNACKED_PACKETS: usize = 32;

#[derive(thiserror::Error, Debug)]
pub enum RpcError {
    #[error("connect error {0:?}")]
    Connect(io::Error),
    #[error("binding error {0:?}")]
    Bind(io::Error),
    #[error("receive error {0:?}")]
    Receive(io::Error),
    #[error("receive error {0:?}")]
    Send(io::Error),

    #[error("from bytes error {0:?}")]
    FromBytes(PodCastError),

    #[error("histogram error {0:?}")]
    Histogram(&'static str),
    #[error("request timed out")]
    Timeout,
    #[error("payload too large at {0} bytes")]
    PayloadTooLarge(usize),
    #[error("not connected")]
    NotConnected,
}

#[async_trait::async_trait]
pub trait Connection {
    fn is_connected(&self) -> bool;
    async fn recv(&mut self) -> Result<Typed<Message>, RpcError>;
    async fn recv_with_timeout(
        &mut self,
        timeout_duration: Duration,
    ) -> Result<Typed<Message>, RpcError>;
    async fn send(&mut self, payload: &[u8]) -> Result<u16, RpcError>;
}

trait Tagged {
    type Tag;
    fn tag(&self) -> Option<Self::Tag>;
}

pub struct Typed<T> {
    bytes: Vec<u8>,
    _pd: PhantomData<T>,
}

/// Wraps a type signature with a bytemuck deserializer.
impl<T> Typed<T>
where
    T: AnyBitPattern + NoUninit + Clone,
{
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            _pd: PhantomData::<T>,
        }
    }

    pub fn try_ref(&self) -> Result<&T, RpcError> {
        bytemuck::try_from_bytes(&self.bytes).map_err(RpcError::FromBytes)
    }

    pub fn try_mut(&mut self) -> Result<&mut T, RpcError> {
        bytemuck::try_from_bytes_mut(&mut self.bytes).map_err(RpcError::FromBytes)
    }
}

pub struct TypedRef<'a, T> {
    bytes: &'a [u8],
    _pd: PhantomData<T>,
}

impl<'a, T> TypedRef<'a, T>
where
    T: AnyBitPattern + NoUninit + Clone,
{
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            _pd: PhantomData::<T>,
        }
    }

    pub fn try_ref(&self) -> Result<&T, RpcError> {
        bytemuck::try_from_bytes(self.bytes).map_err(RpcError::FromBytes)
    }

    pub fn to_owned(&self) -> Result<Typed<T>, RpcError> {
        Ok(Typed::new(self.bytes.to_vec()))
    }
}

#[derive(bytemuck::Pod, bytemuck::Zeroable, Copy, Clone, PartialEq, Debug)]
#[repr(C)]
pub struct Message {
    pub seq: u16,
    pub ack: u16,
    pub ack_bits: u32,
    pub payload: [u8; PAYLOAD_LEN],
}

pub fn next_seq(current: u16) -> u16 {
    if current == std::u16::MAX {
        return 0;
    }
    current + 1
}

impl Message {
    pub fn new(seq: u16, ack: u16, ack_bits: u32, bytes: &[u8]) -> Self {
        let mut payload = [0; PAYLOAD_LEN];
        payload[..bytes.len()].copy_from_slice(bytes);
        Self {
            seq,
            ack,
            ack_bits,
            payload,
        }
    }
}
