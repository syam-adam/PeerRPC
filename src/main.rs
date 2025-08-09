pub mod proto {
    tonic::include_proto!("p2p");
}

use proto::peer_server::{Peer, PeerServer};
use proto::{AnnounceRequest, AnnounceResponse};

use tonic::{transport::Server, Request, Response, Status};
use tracing::info;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct MyPeer {
    addr: String,
    known_peers: Arc<Mutex<Vec<String>>>,
}

#[tonic::async_trait]
impl Peer for MyPeer {
    async fn announce(
        &self,
        request: Request<AnnounceRequest>,
    ) -> Result<Response<AnnounceResponse>, Status> {
        let from = request.into_inner().from_addr;
        info!("Got announcement from {}", from);

        // Save peer to list if new
        let mut peers = self.known_peers.lock().unwrap();
        if !peers.contains(&from) && from != self.addr {
            peers.push(from.clone());
            info!("Added {} to known peers", from);
        }

        Ok(Response::new(AnnounceResponse {
            message: format!("Hello {}, I am {}", from, self.addr),
        }))
    }
}

async fn run_server(peer: MyPeer, addr: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let socket: SocketAddr = addr.parse()?;
    info!("Starting server at {}", addr);

    Server::builder()
        .add_service(PeerServer::new(peer))
        .serve(socket)
        .await?;

    Ok(())
}

async fn announce_to(target: &str, from_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = proto::peer_client::PeerClient::connect(format!("http://{}", target)).await?;
    let response = client
        .announce(AnnounceRequest {
            from_addr: from_addr.to_string(),
        })
        .await?;
    info!("Response from {}: {}", target, response.into_inner().message);
    Ok(())
}

fn load_peer_list() -> Vec<String> {
    std::fs::read_to_string("peers.txt")
        .unwrap_or_default()
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <port>", args[0]);
        std::process::exit(1);
    }

    let port = &args[1];
    let my_addr = format!("127.0.0.1:{}", port);

    let peer_list = load_peer_list();

    let peer_struct = MyPeer {
        addr: my_addr.clone(),
        known_peers: Arc::new(Mutex::new(peer_list.clone())),
    };

    // Start server in background
    let server_task = tokio::spawn(run_server(peer_struct, my_addr.clone()));

    // Delay to ensure server is ready before connecting
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Auto-announce to all peers in peers.txt except self
    for peer_addr in peer_list {
        if peer_addr != my_addr {
            let my_clone = my_addr.clone();
            tokio::spawn(async move {
                if let Err(e) = announce_to(&peer_addr, &my_clone).await {
                    eprintln!("Failed to announce to {}: {}", peer_addr, e);
                }
            });
        }
    }

    let _ = server_task.await?;
    Ok(())
}
