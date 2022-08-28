use std::{
    collections::VecDeque,
    io,
    marker::PhantomData,
    mem::size_of,
    time::{Duration, Instant},
};

use async_io::Timer;
use bitvec::view::BitView;
use bytemuck::{AnyBitPattern, NoUninit, PodCastError};
use futures_lite::FutureExt;
use histogram::Histogram;

const MAX_PAYLOAD_LEN: usize = 16;
const MSG_LEN: usize = size_of::<Message>();
const MAX_UNACKED_PACKETS: usize = 32;

#[derive(thiserror::Error, Debug)]
pub enum RpcError {
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
}

pub struct Peer {
    id: u8,
    seq: u16,
    remote_seq: u16,
    bind: String,
    dest: String,
    bytes_sent: usize,
    socket: async_net::UdpSocket,
    rtt: Histogram,
    send_queue: VecDeque<(u16, Instant, bool)>,
    recv_queue: VecDeque<(u16, Instant, bool)>,
    final_ackd_sequences: Vec<u16>,
}

impl Peer {
    pub async fn bind(addr: &str, dest: &str) -> Result<Self, RpcError> {
        let socket = async_net::UdpSocket::bind(addr)
            .await
            .map_err(RpcError::Bind)?;
        Ok(Self {
            id: 123, // TODO think about id
            seq: 0,
            remote_seq: 0,
            bind: addr.to_string(),
            dest: dest.to_string(),
            socket,
            bytes_sent: 0,
            rtt: Histogram::new(),
            send_queue: VecDeque::new(),
            recv_queue: VecDeque::new(),
            final_ackd_sequences: Vec::new(),
        })
    }

    fn next_seq(&mut self) {
        self.seq = advance_maybe_wrap(self.seq);
    }

    pub async fn recv(&mut self) -> Result<Typed<Message>, RpcError> {
        let mut buf = vec![0; MSG_LEN];
        let num_bytes = self
            .socket
            .recv(&mut buf)
            .or(async {
                Timer::after(Duration::from_millis(1000)).await;
                Err(io::ErrorKind::TimedOut.into())
            })
            .await
            .map_err(RpcError::Receive)?;

        let bytes = buf[..num_bytes].to_vec();
        let msg_wrap = Typed::new(bytes);
        let msg: &Message = msg_wrap.try_ref()?;

        self.push_recv_queue(msg.seq);

        // if the remote sequence is higher, we set the remote sequence from the message.
        if let Some(_higher) = wrapping_sub(self.remote_seq, msg.seq) {
            self.remote_seq = msg.seq;
        }

        self.handle_message_acks(msg)?;

        Ok(msg_wrap)
    }

    fn handle_message_acks(&mut self, msg: &Message) -> Result<(), RpcError> {
        let ack_bits = msg.ack_bits.view_bits::<bitvec::prelude::Lsb0>();
        for (index, bit) in ack_bits.iter().enumerate() {
            if !*bit {
                continue;
            }
            if let Some((seq, req_start, ackd @ false)) = self.send_queue.get_mut(index) {
                let elapsed_micros = req_start.elapsed().as_micros();
                *ackd = *bit;
                self.final_ackd_sequences.push(*seq);
                self.rtt
                    .increment(elapsed_micros as u64)
                    .map_err(RpcError::Histogram)?;
            }
        }
        Ok(())
    }

    pub fn recvd_ack_bits(&self, ack: u16) -> u32 {
        let mut ack_bits = 0u32;
        let bits = ack_bits.view_bits_mut::<bitvec::prelude::Lsb0>();
        for n in 0..MAX_UNACKED_PACKETS {
            if !(ack >= n as u16) {
                continue;
            }
            if self
                .recv_queue
                .iter()
                .any(|(seq, _, _)| *seq == ack - n as u16)
            {
                bits.set(n, true);
            }
        }
        ack_bits
    }

    pub async fn send(&mut self, payload: &[u8]) -> Result<u16, RpcError> {
        let msg = Message::new(
            self.seq,
            self.remote_seq,
            self.recvd_ack_bits(self.remote_seq),
            payload,
        );
        self.push_send_queue(msg.seq);
        self.next_seq();
        let bytes = bytemuck::bytes_of(&msg);
        self.bytes_sent += self
            .socket
            .send_to(bytes, &self.dest)
            .await
            .map_err(RpcError::Send)?;
        Ok(msg.seq)
    }

    // Mark a message as sent, to be used when reading from ack_bits
    pub fn push_send_queue(&mut self, seq: u16) {
        if self.send_queue.len() == MAX_UNACKED_PACKETS {
            self.send_queue.pop_front();
        }
        self.send_queue.push_back((seq, Instant::now(), false));
    }

    // mark a message as recieved, to be used in generation of ack_bits
    pub fn push_recv_queue(&mut self, seq: u16) {
        if self.recv_queue.len() == MAX_UNACKED_PACKETS {
            self.recv_queue.pop_front();
        }
        self.recv_queue.push_back((seq, Instant::now(), true));
    }

    pub fn log_status(&self) {
        println!(
            "\n* status {} seq: {}, remote seq: {}\nsend queue\n{:?}\nrecv queue\n{:?}\nfinal_ackd_sequences({}, missed {}):\n{:?}",
            self.bind,
            self.seq,
            self.remote_seq,
            self.send_queue
                .iter()
                .map(|(seq, instant, ackd)| (
                    seq,
                    instant.elapsed().as_millis(),
                    if *ackd { 1 } else { 0 }
                ))
                .collect::<Vec<_>>(),
            self.recv_queue
                .iter()
                .map(|(seq, instant, ackd)| (
                    seq,
                    instant.elapsed().as_millis(),
                    if *ackd { 1 } else { 0 }
                ))
                .collect::<Vec<_>>(),
            self.final_ackd_sequences.len(),
            self.seq - self.final_ackd_sequences.len() as u16,
            self.final_ackd_sequences,
        );
    }
}

trait Tagged {
    type Tag;
    fn tag(&self) -> Option<Self::Tag>;
}

pub struct Typed<T> {
    bytes: Vec<u8>,
    _pd: PhantomData<T>,
}

impl<T> Typed<T>
where
    T: AnyBitPattern + NoUninit + Clone,
{
    fn new(bytes: Vec<u8>) -> Self {
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
        bytemuck::try_from_bytes(&self.bytes).map_err(RpcError::FromBytes)
    }

    pub fn to_owned(&self) -> Result<Typed<T>, RpcError> {
        Ok(Typed::new(self.bytes.to_vec()))
    }
}

#[derive(bytemuck::Pod, bytemuck::Zeroable, Copy, Clone, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Message {
    seq: u16,
    ack: u16,
    ack_bits: u32,
    payload: [u8; MAX_PAYLOAD_LEN],
}

fn advance_maybe_wrap(seq: u16) -> u16 {
    if seq == std::u16::MAX {
        0
    } else {
        seq + 1
    }
}

fn wrapping_sub(seq: u16, maybe_next: u16) -> Option<u16> {
    const HALF_MAX: u16 = std::u16::MAX / 2;
    // if the number appears to have wrapped
    if seq.saturating_sub(maybe_next) > HALF_MAX {
        Some(std::u16::MAX - seq + maybe_next)
    } else if maybe_next > seq {
        Some(maybe_next.saturating_sub(seq))
    } else {
        None
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
enum MessageType {
    Rpc = 1,
    Ack = 2,
}

pub fn next_seq(current: u16) -> u16 {
    if current == std::u16::MAX {
        return 0;
    }
    current + 1
}

impl Message {
    pub fn new(seq: u16, ack: u16, ack_bits: u32, bytes: &[u8]) -> Self {
        let mut payload = [0; MAX_PAYLOAD_LEN];
        payload[..bytes.len()].copy_from_slice(bytes);
        Self {
            seq,
            ack,
            ack_bits,
            payload,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[smol_potat::test]
    async fn playing_with_udp1() {
        let start = Instant::now();
        let p1_task = std::thread::spawn(|| {
            futures_lite::future::block_on(async move {
                let mut p1 = Peer::bind("127.0.0.1:8082", "127.0.0.1:8083")
                    .await
                    .unwrap();
                for x in 0..100 {
                    for _ in 0..10 {
                        let seq = p1.send(b"hello world").await.unwrap();
                    }
                    let recvd = match p1.recv().await {
                        Ok(msg_recvd) => msg_recvd,
                        Err(err) => {
                            println!("failed to send {err:?}");
                            continue;
                        }
                    };
                    let msg = recvd.try_ref().unwrap();
                    assert_eq!(
                        &Message::new(msg.seq, msg.ack, msg.ack_bits, b"hey there guy"),
                        msg
                    );
                    async_io::Timer::after(Duration::from_millis(10)).await;
                }
                p1.log_status();
                p1
            })
        });
        let p2_task = std::thread::spawn(|| {
            futures_lite::future::block_on(async move {
                let mut p2 = Peer::bind("127.0.0.1:8083", "127.0.0.1:8082")
                    .await
                    .unwrap();
                for x in 0..200 {
                    p2.send(b"hey there guy").await.unwrap();
                    let recvd = match p2.recv().await {
                        Ok(msg_recvd) => msg_recvd,
                        Err(err) => {
                            println!("failed to send, error: {err:?}");
                            continue;
                        }
                    };
                    let msg = recvd.try_ref().unwrap();
                    assert_eq!(
                        &Message::new(msg.seq, msg.ack, msg.ack_bits, b"hello world"),
                        msg
                    );
                    async_io::Timer::after(Duration::from_millis(20)).await;
                }
                p2.log_status();
                p2
            })
        });

        std::thread::sleep(Duration::from_millis(1000));

        let (p1, p2) = (p1_task.join().unwrap(), p2_task.join().unwrap());
        println!("elapsed {:?}", start.elapsed());

        println!(
            "p1 rtt {:?}, mean {:?}, min {:?}, max {:?}",
            p1.rtt,
            p1.rtt.mean(),
            p1.rtt.minimum(),
            p1.rtt.maximum()
        );
        println!(
            "p2 rtt {:?}, mean {:?}, min {:?}, max {:?}",
            p2.rtt,
            p2.rtt.mean(),
            p2.rtt.minimum(),
            p2.rtt.maximum()
        );
    }

    #[smol_potat::test]
    async fn playing_with_udp2() {
        let mut p1 = Peer::bind("127.0.0.1:8080", "127.0.0.1:8081")
            .await
            .unwrap();
        let mut p2 = Peer::bind("127.0.0.1:8081", "127.0.0.1:8080")
            .await
            .unwrap();

        for x in 0..100 {
            p1.send(b"hello world").await.unwrap();
            let recvd = match p2.recv().await {
                Ok(msg_recvd) => msg_recvd,
                Err(err) => {
                    println!("failed to send {err:?}");
                    continue;
                }
            };
            assert_eq!(
                &Message::new(
                    x,
                    recvd.try_ref().unwrap().ack,
                    p1.recvd_ack_bits(p1.remote_seq),
                    b"hello world"
                ),
                recvd.try_ref().unwrap(),
            );
            p2.send(b"hey there guy").await.unwrap();
            match p1.recv().await {
                Ok(ack) => {
                    assert_eq!(
                        &Message::new(
                            x,
                            ack.try_ref().unwrap().ack,
                            p2.recvd_ack_bits(p2.remote_seq),
                            b"hey there guy"
                        ),
                        ack.try_ref().unwrap()
                    );
                }
                Err(err) => {
                    println!("p1 did not receive ack for {x} {err:?}")
                }
            }
        }

        assert_eq!(p1.seq, 100);
        assert_eq!(p2.seq, 100);

        println!(
            "rtt {:?}, mean {:?}, min {:?}, max {:?}",
            p1.rtt,
            p1.rtt.mean(),
            p1.rtt.minimum(),
            p1.rtt.maximum()
        );
    }

    #[test]
    fn ack_bits() {
        let mut i: u32 = 0b00000000000000000000000000000001;
        for _ in 0..32 {
            i <<= 1;
            println!("{i}");
        }
    }

    #[test]
    fn bitor() {
        let mut i: u32 = 0b10001000111000000010100000100010;
        i |= 1;
        assert_eq!(i, 0b10001000111000000010100000100011);
    }

    #[test]
    fn shiftoff() {
        let mut i: u32 = 0b10001000111000000010100000100010;
        i <<= 1;
        assert_eq!(i, 0b00010001110000000101000001000100);
        i <<= 1;
        assert_eq!(i, 0b00100011100000001010000010001000);
    }

    #[test]
    fn wrapping_u16s() {
        assert_eq!(wrapping_sub(std::u16::MAX - 5, 5), Some(10));
        assert_eq!(wrapping_sub(std::u16::MAX - 500, 5), Some(505));
        assert_eq!(wrapping_sub(0, std::u16::MAX), Some(std::u16::MAX));
        assert_eq!(
            wrapping_sub(std::u16::MAX / 2, (std::u16::MAX / 2) + 1),
            Some(1)
        );
        assert_eq!(
            wrapping_sub((std::u16::MAX / 2) - 1, (std::u16::MAX / 2) + 1),
            Some(2)
        );
        assert_eq!(wrapping_sub(5, 1), None);
    }
}
