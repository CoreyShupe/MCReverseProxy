use drax::prelude::BytesMut;
use drax::transport::buffered_reader::DraxTransportPipeline;
use drax::transport::pipeline::ChainProcessor;
use drax::transport::{DraxTransport, TransportProcessorContext};
use std::io::Cursor;
use std::sync::Arc;
use tokio::net::{TcpStream, ToSocketAddrs};

use drax::transport::buffered_writer::{FrameSizeAppender, GenericWriter};
use drax::transport::frame::FrameEncoder;
use drax::{link, VarInt};

#[derive(drax_derive::DraxTransport, Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
#[drax(key = {match VarInt})]
pub enum NextState {
    Handshaking,
    Status,
    Login,
}

#[derive(drax_derive::DraxTransport, Debug, Clone)]
pub struct Handshake {
    pub protocol_version: VarInt,
    #[drax(limit = 255)]
    pub server_address: String,
    pub server_port: u16,
    pub next_state: NextState,
}

#[derive(drax_derive::DraxTransport, Debug, Clone)]
#[drax(key = {match VarInt})]
pub enum HandshakeWrapper {
    Handshake(Handshake),
}

pub const MC_BUFFER_CAPACITY: usize = 2097154; // static value from wiki.vg

pub struct MinecraftReverseProxy {
    inbound: TcpStream,
    outbound: TcpStream,
    overflow: Option<BytesMut>,
}

struct MCHandshakeProcessor;
impl ChainProcessor for MCHandshakeProcessor {
    type Input = Vec<u8>;
    type Output = Handshake;

    fn process<'a>(
        &'a self,
        context: &'a mut TransportProcessorContext,
        input: Self::Input,
    ) -> drax::transport::Result<Self::Output> {
        let mut cursor = Cursor::new(input);
        let handshake = HandshakeWrapper::read_from_transport(context, &mut cursor)?;
        match handshake {
            HandshakeWrapper::Handshake(hs) => Ok(hs),
        }
    }
}

impl MinecraftReverseProxy {
    pub async fn read_handshake(&mut self) -> drax::transport::Result<Handshake> {
        let mut pipeline = DraxTransportPipeline::new(
            Arc::new(MCHandshakeProcessor),
            BytesMut::with_capacity(MC_BUFFER_CAPACITY),
        );
        let handshake = pipeline
            .read_transport_packet(&mut TransportProcessorContext::new(), &mut self.inbound)
            .await?;
        self.overflow = Some(pipeline.into_inner());
        Ok(handshake)
    }

    pub async fn write_handshake(&mut self, handshake: Handshake) -> drax::transport::Result<()> {
        let chain = link!(GenericWriter, FrameEncoder::new(-1), FrameSizeAppender);
        let encoded: Vec<u8> = chain.process(
            &mut TransportProcessorContext::new(),
            Box::new(HandshakeWrapper::Handshake(handshake)),
        )?;
        tokio::io::copy(&mut Cursor::new(encoded), &mut self.outbound).await?;
        if let Some(extra_buffer) = self.overflow.take() {
            extra_buffer.len();
            tokio::io::copy(&mut Cursor::new(extra_buffer), &mut self.outbound).await?;
        }
        Ok(())
    }

    pub fn spawn_proxy(self) {
        tokio::spawn(async move {
            let Self {
                mut inbound,
                mut outbound,
                ..
            } = self;
            let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound)
                .await
                .ok();
        });
    }
}

pub async fn connect_basic<T: ToSocketAddrs>(addr: T) -> std::io::Result<TcpStream> {
    TcpStream::connect(addr).await
}

pub async fn proxy_connection(
    inbound: TcpStream,
    outbound: TcpStream,
    target_address: String,
) -> drax::transport::Result<()> {
    let mut proxy = MinecraftReverseProxy {
        inbound,
        outbound,
        overflow: None,
    };
    let handshake = proxy.read_handshake().await?;
    if handshake.next_state == NextState::Login {
        log::info!("New player connected.");
    }
    proxy
        .write_handshake(Handshake {
            protocol_version: handshake.protocol_version,
            server_address: target_address,
            server_port: handshake.server_port,
            next_state: handshake.next_state,
        })
        .await?;
    proxy.spawn_proxy();
    Ok(())
}
