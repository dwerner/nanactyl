//! Plugin: `net_sync_plugin`
//! Implements a plugin (see crates/plugin-loader) for prototyping network sync.
//! TODO:
//!     - move connection impl and pumping here.
//!     - hone an api for world state -> net sync update transition.

use std::collections::VecDeque;
use std::io;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use async_io::Timer;
use bitvec::view::BitView;
use bytemuck::PodCastError;
use futures_lite::FutureExt;
use histogram::Histogram;
use input::wire::InputState;
use logger::{error, info, LogLevel};
use network::{Connection, Message, RpcError, Typed, MAX_UNACKED_PACKETS, MSG_LEN, PAYLOAD_LEN};
use wire::{WirePosition, WireThing, WorldUpdate};
use world::thing::Thing;
use world::{Vec3, World, WorldError, WorldLockAndControllerState};

const NUM_UPDATES_PER_MSG: u32 = 96;

#[derive(thiserror::Error, Debug)]
enum PluginError {
    #[error("Error pod casting update from bytes {0:?} len {1}")]
    FromBytes(PodCastError, usize),
    #[error("world error {0}")]
    World(#[from] WorldError),
}

#[no_mangle]
pub extern "C" fn load(state: &mut WorldLockAndControllerState) {
    info!(
        state.logger,
        "reloaded net sync plugin ({})!", state.world.stats.updates
    );
    let connection = match state.world.config.maybe_server_addr {
        Some(addr) => {
            futures_lite::future::block_on(async move {
                let mut server = Peer::bind_dest("0.0.0.0:12001", &addr.to_string())
                    .await
                    .unwrap();

                // initial message to client
                server.send(b"moar plz").await.unwrap();
                server
            })
        }
        None => {
            let logger = LogLevel::Info.logger();
            // We will run as a server, accepting new connections.
            futures_lite::future::block_on(async move {
                let addr = "0.0.0.0:12002";
                info!(logger, "binding addr {addr}");
                let mut client = Peer::bind_only(addr).await.unwrap();
                client.recv().await.unwrap();
                client
            })
        }
    };

    state.world.connection =
        Some(Box::new(connection) as Box<dyn Connection + Send + Sync + 'static>);
}

#[no_mangle]
pub extern "C" fn update(s: &mut WorldLockAndControllerState, _dt: &Duration) {
    let logger = s.logger.sub("net_sync_plugin-update");
    // TODO: fix sized net sync issue (try > NUM_UPDTES_PER_MSG items)
    if s.world.is_server() && s.world.things.len() >= 96 {
        match futures_lite::future::block_on(pump_connection_as_server(&mut s.world)) {
            Ok(controller_state) => {
                // TODO: support N controllers, or just one per client?
                s.world.set_client_controller_state(controller_state[0]);
                let new_server_states = s.controller_state[0];
                s.world.set_server_controller_state(new_server_states);
            }
            Err(err) => error!(logger, "error pumping server connection {err:?}"),
        }
    } else {
        match futures_lite::future::block_on(pump_connection_as_client(
            &mut s.world,
            &*s.controller_state,
        )) {
            Err(PluginError::World(WorldError::Network(network::RpcError::Receive(kind))))
                if kind.kind() == std::io::ErrorKind::TimedOut => {}
            Err(err) => {
                error!(logger, "error in client connection {err:?}");
            }
            _ => (),
        }
    };
}

#[no_mangle]
pub extern "C" fn unload(state: &mut WorldLockAndControllerState) {
    info!(
        state.logger,
        "unloaded net sync plugin ({})...", state.world.stats.updates
    );
    state.world.connection.take();
}

async fn pump_connection_as_server(s: &mut World) -> Result<[InputState; 2], PluginError> {
    // 1. construct a group of all world state.
    let packet = s
        .things
        .iter()
        .enumerate()
        .map(|(idx, thing)| {
            let id = idx as u32;
            let thing: WireThing = thing.into();
            let p = &s.facets.physical[thing.phys as usize];
            WorldUpdate {
                id,
                thing,
                position: WirePosition(p.position.x, p.position.y, p.position.z),
                y_rotation: p.angles.y,
            }
        })
        .take(NUM_UPDATES_PER_MSG as usize)
        .collect::<Vec<_>>();
    // 2. Compress that
    let compressed = wire::compress_world_updates(&packet)?;
    let _seq = s.connection.as_mut().unwrap().send(&compressed).await;
    let client_controller_data = s
        .connection
        .as_mut()
        .unwrap()
        .recv_with_timeout(Duration::from_millis(1))
        .await
        .map_err(WorldError::Network)?;

    let payload = client_controller_data
        .try_ref()
        .map_err(WorldError::Network)?
        .payload;
    let len: &u16 = bytemuck::from_bytes(&payload[0..2]);
    let len = *len;
    if len + 2 > payload.len() as u16 {
        return Err(PluginError::World(WorldError::Network(
            network::RpcError::Receive(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid payload length - {}/{}", len, payload.len()),
            )),
        )));
    }
    let cast: &[InputState; 2] = bytemuck::try_from_bytes(&payload[2..2 + len as usize])
        .map_err(|err| PluginError::FromBytes(err, payload.len()))?;
    Ok(*cast)
}

async fn pump_connection_as_client(
    s: &mut World,
    controllers: &[InputState],
) -> Result<(), PluginError> {
    let logger = s.logger.sub("pump_connection_as_client");
    let mut last_pkt = None;

    // read until we timeout with 0ms, because we want to know only the very latest
    // packet
    let data = 'recv: loop {
        match s
            .connection
            .as_mut()
            .unwrap()
            .recv_with_timeout(Duration::from_millis(0))
            .await
        {
            Ok(pkt) => {
                last_pkt = Some(pkt);
            }
            Err(network::RpcError::Receive(err)) => {
                if err.kind() == std::io::ErrorKind::TimedOut {
                    if let Some(last_pkt) = last_pkt {
                        break 'recv last_pkt;
                    }
                } else {
                    return Err(PluginError::World(WorldError::Network(
                        network::RpcError::Receive(err),
                    )));
                }
            }
            Err(other) => {
                return Err(PluginError::World(WorldError::Network(other)));
            }
        }
    };

    let decompressed_updates = wire::decompress_world_updates(
        &data.try_ref().map_err(WorldError::UpdateFromBytes)?.payload,
    )?;

    for wire::WorldUpdate {
        id,
        thing,
        position,
        y_rotation,
    } in decompressed_updates
    {
        let thing: Thing = thing.into();
        match s.things.get_mut(id as usize) {
            Some(t) => *t = thing,
            None => error!(logger, "thing not found at index {id}"),
        };
        match s.facets.physical.get_mut(id as usize) {
            Some(phys) => {
                phys.position = Vec3::new(position.0, position.1, position.2);
                phys.angles.y = y_rotation;
            }
            None => error!(logger, "no physical facet at index {}", position.0),
        }
    }

    let mut msg_bytes = vec![];

    // TODO: support more controllers in another manner
    let mut controllers_limited: [InputState; 2] = Default::default();
    controllers_limited.copy_from_slice(controllers);

    let controller_state_bytes = bytemuck::bytes_of(&controllers_limited);
    let len = controller_state_bytes.len().min(PAYLOAD_LEN);
    msg_bytes.extend(bytemuck::bytes_of(&(len as u16)));
    msg_bytes.extend(controller_state_bytes);

    // TODO: make use of this result properly
    let _ = s.connection.as_mut().unwrap().send(&msg_bytes).await;
    Ok(())
}

pub mod wire {

    use bytemuck::{Pod, Zeroable};
    use world::thing::{self, Thing};

    use super::*;

    #[derive(Debug, Default, Copy, Clone, Pod, Zeroable)]
    #[repr(C)]
    pub struct WorldUpdate {
        pub id: u32,
        pub thing: WireThing,
        pub position: WirePosition,

        // FOR RIGHT NOW, only support y axis rotation
        pub y_rotation: f32,
    }

    impl From<&Thing> for WireThing {
        fn from(thing: &Thing) -> Self {
            match thing.facets {
                thing::ThingType::Camera { phys, camera } => Self {
                    tag: 0,
                    phys: phys.into(),
                    facet: camera.into(),
                    _pad: 0,
                },
                thing::ThingType::GraphicsObject { phys, model } => Self {
                    tag: 1,
                    phys: phys.into(),
                    facet: model.into(),
                    _pad: 0,
                },
            }
        }
    }

    impl From<WireThing> for Thing {
        fn from(wt: WireThing) -> Self {
            match wt {
                WireThing {
                    tag: 0,
                    phys,
                    facet,
                    ..
                } => Thing::camera(phys.into(), facet.into()),
                WireThing {
                    tag: 1,
                    phys,
                    facet,
                    ..
                } => Thing::model(phys.into(), facet.into()),
                _ => unreachable!(),
            }
        }
    }

    #[derive(Debug, Default, Copy, Clone, Pod, Zeroable)]
    #[repr(C)]
    pub struct WireThing {
        pub tag: u8,
        _pad: u8,
        pub facet: u16,
        pub phys: u32,
    }

    #[derive(Debug, Default, Copy, Clone, Pod, Zeroable)]
    #[repr(C)]
    pub struct WirePosition(pub f32, pub f32, pub f32);

    const ZSTD_LEVEL: i32 = 3;

    /// Compress an update with zstd.
    pub(crate) fn compress_world_updates(values: &[WorldUpdate]) -> Result<Vec<u8>, PluginError> {
        let mut sized: [WorldUpdate; NUM_UPDATES_PER_MSG as usize] =
            [WorldUpdate::default(); NUM_UPDATES_PER_MSG as usize];
        sized.copy_from_slice(values);
        let mut compressed_bytes = vec![];
        let read_bytes = bytemuck::bytes_of(&sized);
        let encoded =
            zstd::encode_all(read_bytes, ZSTD_LEVEL).map_err(WorldError::UpdateCompression)?;
        let len = encoded.len();
        let len = len.min(PAYLOAD_LEN) as u16;
        let len = bytemuck::bytes_of(&len);
        compressed_bytes.extend(len);
        compressed_bytes.extend(encoded);
        Ok(compressed_bytes)
    }

    /// Decompress an update using zstd.
    pub(crate) fn decompress_world_updates(
        compressed: &[u8],
    ) -> Result<Vec<WorldUpdate>, PluginError> {
        let mut decoded_bytes = vec![];
        let len: &u16 = bytemuck::from_bytes(&compressed[0..2]);
        let len = *len;
        let len = len.min(PAYLOAD_LEN as u16);
        let decoded = zstd::decode_all(&compressed[2..(2 + len as usize).min(compressed.len())])
            .map_err(WorldError::UpdateDecompression)?;
        decoded_bytes.extend(decoded);
        let updates: &[WorldUpdate; NUM_UPDATES_PER_MSG as usize] =
            bytemuck::try_from_bytes(&decoded_bytes)
                .map_err(|err| PluginError::FromBytes(err, decoded_bytes.len()))?;
        Ok(updates.to_vec())
    }

    #[cfg(test)]
    mod tests {

        use logger::debug;
        use world::thing::{GfxIndex, PhysicalIndex};

        use super::*;

        #[test]
        fn test_compression_roundtrip() {
            let values = (0..NUM_UPDATES_PER_MSG)
                .map(|i| {
                    let physical = PhysicalIndex::from(i);
                    let model = GfxIndex::from(i);
                    let model = Thing::model(physical, model);
                    let wt: WireThing = (&model).into();
                    let wpos = WirePosition(i as f32, i as f32, i as f32);
                    WorldUpdate {
                        id: i,
                        thing: wt,
                        position: wpos,
                        y_rotation: 0.0,
                    }
                })
                .collect::<Vec<_>>();

            let compressed_bytes = compress_world_updates(&values).unwrap();
            debug!(
                LogLevel::Info.logger(),
                "compressed_bytes {}",
                compressed_bytes.len()
            );
            let decompressed = decompress_world_updates(&compressed_bytes).unwrap();
            assert_eq!(values.len(), decompressed.len());
        }
    }
}

pub struct Peer {
    _id: u8, // TODO
    seq: u16,
    remote_seq: u16,
    dest: Option<SocketAddr>,
    bytes_sent: usize,
    socket: async_net::UdpSocket,
    pub rtt_micros: Histogram,
    send_queue: VecDeque<(u16, Instant, bool)>,
    recv_queue: VecDeque<(u16, Instant, bool)>,
    own_final_ackd_sequences: Vec<u16>,
}

#[async_trait::async_trait]
impl Connection for Box<Peer> {
    fn is_connected(&self) -> bool {
        self.dest.is_some()
    }

    /// Wait forever to receive a datagram.
    async fn recv(&mut self) -> Result<Typed<Message>, RpcError> {
        self.recv_with_optional_timeout(None).await
    }

    /// Receive a datagram or timeout.
    async fn recv_with_timeout(
        &mut self,
        timeout_duration: Duration,
    ) -> Result<Typed<Message>, RpcError> {
        self.recv_with_optional_timeout(Some(timeout_duration))
            .await
    }

    async fn send(&mut self, payload: &[u8]) -> Result<u16, RpcError> {
        if payload.len() > PAYLOAD_LEN {
            return Err(RpcError::PayloadTooLarge(payload.len()));
        }
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
            .send_to(bytes, &self.dest.ok_or(RpcError::NotConnected)?)
            .await
            .map_err(RpcError::Send)?;
        Ok(msg.seq)
    }
}

impl Peer {
    fn next_seq(&mut self) {
        self.seq = advance_maybe_wrap(self.seq);
    }

    pub async fn bind_only(addr: &str) -> Result<Box<Self>, RpcError> {
        let socket = async_net::UdpSocket::bind(addr)
            .await
            .map_err(RpcError::Bind)?;

        Ok(Box::new(Self {
            _id: 123,
            seq: 0,
            remote_seq: 0,
            dest: None,
            socket,
            bytes_sent: 0,
            rtt_micros: Histogram::new(),
            send_queue: VecDeque::new(),
            recv_queue: VecDeque::new(),
            own_final_ackd_sequences: Vec::new(),
        }))
    }

    pub async fn bind_dest(addr: &str, dest: &str) -> Result<Box<Self>, RpcError> {
        let socket = async_net::UdpSocket::bind(addr)
            .await
            .map_err(RpcError::Bind)?;
        Ok(Box::new(Self {
            _id: 123, // TODO think about id
            seq: 0,
            remote_seq: 0,
            dest: Some(dest.parse().unwrap()),
            socket,
            bytes_sent: 0,
            rtt_micros: Histogram::new(),
            send_queue: VecDeque::new(),
            recv_queue: VecDeque::new(),
            own_final_ackd_sequences: Vec::new(),
        }))
    }

    pub async fn recv_with_optional_timeout(
        &mut self,
        maybe_timeout_duration: Option<Duration>,
    ) -> Result<Typed<Message>, RpcError> {
        let mut buf = vec![0; MSG_LEN];

        let num_bytes = if self.dest.is_none() {
            let (num_bytes, addr) = match maybe_timeout_duration {
                Some(timeout_duration) => {
                    self.socket
                        .recv_from(&mut buf)
                        .or(async {
                            Timer::after(timeout_duration).await;
                            Err(io::ErrorKind::TimedOut.into())
                        })
                        .await
                }
                None => self.socket.recv_from(&mut buf).await,
            }
            .map_err(RpcError::Receive)?;
            self.socket
                .connect(&addr)
                .await
                .map_err(RpcError::Connect)?;
            self.dest = Some(addr);
            num_bytes
        } else {
            match maybe_timeout_duration {
                Some(timeout_duration) => {
                    self.socket
                        .recv(&mut buf)
                        .or(async {
                            Timer::after(timeout_duration).await;
                            Err(io::ErrorKind::TimedOut.into())
                        })
                        .await
                }
                None => self.socket.recv(&mut buf).await,
            }
            .map_err(RpcError::Receive)?
        };

        let bytes = buf[..num_bytes].to_vec();

        let msg_wrap = Typed::new(bytes);
        let msg: &Message = msg_wrap.try_ref()?;

        self.push_recv_queue(msg.seq);

        // if the remote sequence is higher, we set the remote sequence from the
        // message.
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
                *ackd = *bit;
                self.own_final_ackd_sequences.push(*seq);
                self.rtt_micros
                    .increment(req_start.elapsed().as_micros() as u64)
                    .map_err(RpcError::Histogram)?;
            }
        }
        Ok(())
    }

    fn recvd_ack_bits(&self, latest_ack: u16) -> u32 {
        let mut ack_bits = 0u32;
        let bits = ack_bits.view_bits_mut::<bitvec::prelude::Lsb0>();
        for n in 0..MAX_UNACKED_PACKETS {
            if latest_ack < n as u16 {
                continue;
            }
            if self
                .recv_queue
                .iter()
                .any(|(seq, _, _)| *seq == latest_ack - n as u16)
            {
                bits.set(n, true);
            }
        }
        ack_bits
    }

    // Mark a message as sent, to be used when reading from ack_bits
    fn push_send_queue(&mut self, seq: u16) {
        if self.send_queue.len() == MAX_UNACKED_PACKETS {
            self.send_queue.pop_front();
        }
        self.send_queue.push_back((seq, Instant::now(), false));
    }

    // mark a message as recieved, to be used in generation of ack_bits
    fn push_recv_queue(&mut self, seq: u16) {
        if self.recv_queue.len() == MAX_UNACKED_PACKETS {
            self.recv_queue.pop_front();
        }
        self.recv_queue.push_back((seq, Instant::now(), true));
    }
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

#[cfg(test)]
mod tests {

    use std::mem::size_of;

    use super::*;

    // Just a flag for when the size of the message changes. Keep in mind this is a
    // UDP packet.
    #[test]
    fn assert_size_limit() {
        let size = size_of::<Message>();
        assert!(
            size < 16 + 16 + 32 + PAYLOAD_LEN,
            "Message is too large at {size} bytes."
        );
    }

    #[smol_potat::test]
    async fn test_server_style_connections() {
        let mut server = Peer::bind_only("127.0.0.1:18083").await.unwrap();

        let client_addr = "127.0.0.1:18082";
        let mut client = Peer::bind_dest(client_addr.clone(), "127.0.0.1:18083")
            .await
            .unwrap();

        client.send(b"stuff").await.unwrap();
        server.recv().await.unwrap();

        assert_eq!(
            server.socket.peer_addr().unwrap(),
            client_addr.parse().unwrap()
        )
    }

    #[smol_potat::test]
    async fn test_send_queue() {
        let mut p1 = Peer::bind_dest("127.0.0.1:8084", "127.0.0.1:8085")
            .await
            .unwrap();
        let mut p2 = Peer::bind_dest("127.0.0.1:8085", "127.0.0.1:8084")
            .await
            .unwrap();

        let mut mirrored_queue = Vec::new();
        for _ in 0..5 {
            mirrored_queue.push(p1.send(b"hello").await.unwrap());
        }

        for n in mirrored_queue.iter() {
            let msg = p2.recv().await.unwrap();
            let msg = msg.try_ref().unwrap();
            assert!(mirrored_queue.contains(&msg.seq));

            let ack_bits = p2.recvd_ack_bits(dbg!(p2.remote_seq));
            println!("n{n}: {ack_bits:#034b}");
            println!("recv queue {}", p2.recv_queue.len());
        }

        // Simulate sender side sending 10 more msgs, but none are recvd
        println!("simulating 10 sent messages that are not received.");
        for n in 5..15 {
            // simulate a send
            p1.push_send_queue(n);
            p1.next_seq();

            // print out the ack_bits we would get at this seqence number
            let ack_bits = p2.recvd_ack_bits(dbg!(n));
            println!("n{n}: {ack_bits:#034b}");
        }

        // send 10 more messages
        for _ in 0..10 {
            mirrored_queue.push(p1.send(b"hello").await.unwrap());
        }

        for n in 15..25 {
            let msg = p2.recv().await.unwrap();
            let msg = msg.try_ref().unwrap();
            assert!(mirrored_queue.contains(&msg.seq));

            let ack_bits = p2.recvd_ack_bits(dbg!(n));
            println!("n{n}: {ack_bits:#034b}");
            println!("recv queue {}", p2.recv_queue.len());
        }

        // Ack bits are shifted from lsb to msb as items are ack'd
        // so this bitvec shows the first 5 followed by a gap of 10
        // followed by another 10 acks.
        let expected_ack_bits = 0b00000001111100000000001111111111;

        // receive side only saw 20 packets
        // if MAX_UNACKED_PACKETS is less than 32, we shift over to mask.
        let shift_offset = 32 - MAX_UNACKED_PACKETS;
        assert_eq!(
            p2.recvd_ack_bits(24) << shift_offset,
            expected_ack_bits << shift_offset,
            "left: {:#034b}, right: {:#034b}",
            p2.recvd_ack_bits(24),
            expected_ack_bits << shift_offset,
        );
        assert_eq!(
            p2.recv_queue.len(),
            15.min(MAX_UNACKED_PACKETS),
            "unexpected number of items in recv queue"
        );

        // whereas sender side thinks that it sent 30 total
        assert_eq!(p1.send_queue.len(), 25.min(MAX_UNACKED_PACKETS));

        // a single response needs to be sent, which now contains up to 32 acks.
        p2.send(b"hello").await.unwrap();
        p1.recv().await.unwrap();

        // finally, we walk each bit and ensure that it matches the sender's queue.
        let expected_ack_bits = expected_ack_bits.view_bits::<bitvec::prelude::Lsb0>();
        for (index, bit) in expected_ack_bits[..p1.send_queue.len()].iter().enumerate() {
            let (_, _, ackd) = p1.send_queue[index];
            assert_eq!(bit, ackd, "ack bit not matched for index {index}");
        }
    }

    #[smol_potat::test]
    async fn playing_with_udp1() {
        let start = Instant::now();
        let p1_task = std::thread::spawn(|| {
            futures_lite::future::block_on(async move {
                let mut p1 = Peer::bind_dest("127.0.0.1:9082", "127.0.0.1:8083")
                    .await
                    .unwrap();
                for _x in 0..100 {
                    let _seq = p1.send(b"hello world").await.unwrap();

                    let recvd = match p1.recv_with_timeout(Duration::from_millis(16)).await {
                        Ok(msg_recvd) => msg_recvd,
                        Err(err) => {
                            println!("p1 failed to recv {err:?}");
                            continue;
                        }
                    };
                    let msg = recvd.try_ref().unwrap();
                    assert_eq!(
                        &Message::new(msg.seq, msg.ack, msg.ack_bits, b"hey there"),
                        msg
                    );
                    async_io::Timer::after(Duration::from_millis(16)).await;
                }
                p1.send(b"done").await.unwrap();
                p1.recv().await.unwrap();
                p1
            })
        });
        let p2_task = std::thread::spawn(|| {
            futures_lite::future::block_on(async move {
                let mut p2 = Peer::bind_dest("127.0.0.1:8083", "127.0.0.1:9082")
                    .await
                    .unwrap();
                for _ in 0..101 {
                    p2.send(b"hey there").await.unwrap();
                    let recvd = match p2.recv().await {
                        Ok(msg_recvd) => msg_recvd,
                        Err(err) => {
                            println!("p2 failed to receive, error: {err:?}");
                            continue;
                        }
                    };
                    let msg = recvd.try_ref().unwrap();
                    if let b"done" = &msg.payload[..4] {
                        break;
                    }
                    assert_eq!(
                        &Message::new(msg.seq, msg.ack, msg.ack_bits, b"hello world"),
                        msg
                    );
                    async_io::Timer::after(Duration::from_millis(20)).await;
                }
                println!("p2 done");
                p2
            })
        });

        std::thread::sleep(Duration::from_millis(1000));

        let (p1, p2) = (p1_task.join().unwrap(), p2_task.join().unwrap());
        println!("elapsed {:?}", start.elapsed());

        println!(
            "p1 rtt {:?}, mean {:?}, min {:?}, max {:?}",
            p1.rtt_micros,
            p1.rtt_micros.mean(),
            p1.rtt_micros.minimum(),
            p1.rtt_micros.maximum()
        );
        println!(
            "p2 rtt {:?}, mean {:?}, min {:?}, max {:?}",
            p2.rtt_micros,
            p2.rtt_micros.mean(),
            p2.rtt_micros.minimum(),
            p2.rtt_micros.maximum()
        );
    }

    #[smol_potat::test]
    async fn playing_with_udp2() {
        let mut p1 = Peer::bind_dest("127.0.0.1:8080", "127.0.0.1:8081")
            .await
            .unwrap();
        let mut p2 = Peer::bind_dest("127.0.0.1:8081", "127.0.0.1:8080")
            .await
            .unwrap();

        for x in 0..100 {
            p1.send(b"hello world").await.unwrap();
            let recvd = match p2.recv().await {
                Ok(msg_recvd) => msg_recvd,
                Err(err) => {
                    println!("failed to recv {err:?}");
                    continue;
                }
            };
            assert_eq!(
                &Message::new(
                    x,
                    recvd.try_ref().unwrap().ack,
                    p1.recvd_ack_bits(p1.remote_seq),
                    b"hello world",
                ),
                recvd.try_ref().unwrap(),
            );
            p2.send(b"hey there").await.unwrap();
            match p1.recv().await {
                Ok(ack) => {
                    assert_eq!(
                        &Message::new(
                            x,
                            ack.try_ref().unwrap().ack,
                            p2.recvd_ack_bits(p2.remote_seq),
                            b"hey there"
                        ),
                        ack.try_ref().unwrap()
                    );
                }
                Err(err) => {
                    println!("did not receive ack for {x} {err:?}")
                }
            }
        }

        assert_eq!(p1.seq, 100);
        assert_eq!(p2.seq, 100);

        println!(
            "rtt {:?}, mean {:?}, min {:?}, max {:?}",
            p1.rtt_micros,
            p1.rtt_micros.mean(),
            p1.rtt_micros.minimum(),
            p1.rtt_micros.maximum()
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
