use tokio::net::TcpStream;

pub struct MinecraftReverseProxy {
    inbound: TcpStream,
    outbound: TcpStream,
}

impl MinecraftReverseProxy {
    pub async fn read_handshake() {

    }

    pub async fn write_handshake() {

    }
}
