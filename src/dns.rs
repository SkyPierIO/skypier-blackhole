use crate::config::Upstream;
use crate::{BlocklistManager, Config, Result};
use hickory_client::client::{AsyncClient, ClientHandle};
use hickory_client::udp::UdpClientStream;
use hickory_proto::h2::HttpsClientStreamBuilder;
use hickory_proto::iocompat::AsyncIoTokioAsStd;
use hickory_proto::op::{Message, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::{Name, RData, Record, RecordType};
use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use tokio::net::{TcpStream as TokioTcpStream, UdpSocket};
use tokio::sync::{Mutex, RwLock};

/// DNS server that blocks domains from blocklist and forwards allowed queries
pub struct DnsServer {
    config: Arc<Config>,
    blocklist: Arc<BlocklistManager>,
    stats: Arc<RwLock<Statistics>>,
    /// Cached connection to the upstream resolver, established lazily
    upstream_client: Arc<Mutex<Option<AsyncClient>>>,
}

/// Statistics for monitoring
#[derive(Debug, Default)]
pub struct Statistics {
    pub total_queries: u64,
    pub blocked_queries: u64,
    pub allowed_queries: u64,
    pub start_time: Option<std::time::Instant>,
}

impl DnsServer {
    /// Create a new DNS server instance
    pub fn new(config: Config, blocklist: Arc<BlocklistManager>) -> Result<Self> {
        let stats = Statistics {
            start_time: Some(std::time::Instant::now()),
            ..Default::default()
        };

        Ok(DnsServer {
            config: Arc::new(config),
            blocklist,
            stats: Arc::new(RwLock::new(stats)),
            upstream_client: Arc::new(Mutex::new(None)),
        })
    }

    /// Start the DNS server
    pub async fn start(&self) -> Result<()> {
        let listen_addr = format!(
            "{}:{}",
            self.config.server.listen_addr, self.config.server.listen_port
        );

        tracing::info!(addr = %listen_addr, "Starting DNS server");

        // Bind UDP socket
        let socket = UdpSocket::bind(&listen_addr).await?;
        tracing::info!(proto = "UDP", addr = %listen_addr, "DNS server listening");

        // Create upstream DNS client
        let upstream = self
            .config
            .server
            .upstream_dns
            .first()
            .ok_or_else(|| anyhow::anyhow!("No upstream DNS configured"))?;

        tracing::info!(upstream = %upstream, "Using upstream DNS");

        // Main server loop
        self.run_server(socket).await?;

        Ok(())
    }

    /// Main server loop - handle incoming DNS queries
    async fn run_server(&self, socket: UdpSocket) -> Result<()> {
        let mut buf = vec![0u8; 512]; // Standard DNS packet size
        let socket = Arc::new(socket);

        loop {
            // Receive DNS query
            let (len, src) = match socket.recv_from(&mut buf).await {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to receive from socket");
                    continue;
                }
            };

            tracing::debug!(bytes = len, src = %src, "Received packet");

            // Parse DNS message
            let query = match Message::from_bytes(&buf[..len]) {
                Ok(msg) => msg,
                Err(e) => {
                    tracing::warn!(src = %src, error = %e, "Failed to parse DNS message");
                    continue;
                }
            };

            // Handle query in background task
            let server = self.clone();
            let socket_clone = Arc::clone(&socket);
            tokio::spawn(async move {
                if let Err(e) = server.handle_query(query, src, socket_clone).await {
                    tracing::error!(error = %e, "Error handling query");
                }
            });
        }
    }

    /// Handle a single DNS query
    async fn handle_query(
        &self,
        query: Message,
        src: SocketAddr,
        socket: Arc<UdpSocket>,
    ) -> Result<()> {
        // Update statistics
        {
            let mut stats = self.stats.write().await;
            stats.total_queries += 1;
        }

        // Extract query information
        let query_name = match query.queries().first() {
            Some(q) => q.name().to_utf8(),
            None => {
                tracing::warn!(src = %src, "Query has no questions");
                return Ok(());
            }
        };

        tracing::debug!(src = %src, domain = %query_name, "Query received");

        // Check if domain is blocked
        let is_blocked = self.blocklist.is_blocked(&query_name).await;

        let response = if is_blocked {
            // Domain is blocked
            tracing::info!(domain = %query_name, source_ip = %src.ip(), "blocked");

            {
                let mut stats = self.stats.write().await;
                stats.blocked_queries += 1;
            }

            // Create blocked response
            self.create_blocked_response(&query)
        } else {
            // Domain is allowed - forward to upstream
            tracing::debug!(domain = %query_name, source_ip = %src.ip(), "allowed");

            {
                let mut stats = self.stats.write().await;
                stats.allowed_queries += 1;
            }

            // Forward to upstream DNS
            self.forward_to_upstream(query).await?
        };

        // Send response
        let response_bytes = response.to_bytes()?;
        socket.send_to(&response_bytes, src).await?;

        Ok(())
    }

    /// Create a blocked response based on configuration
    fn create_blocked_response(&self, query: &Message) -> Message {
        let mut response = Message::new();
        response.set_id(query.id());
        response.set_message_type(MessageType::Response);
        response.set_op_code(OpCode::Query);
        response.add_queries(query.queries().to_vec());

        match &self.config.server.blocked_response {
            crate::config::BlockedResponse::Refused => {
                response.set_response_code(ResponseCode::Refused);
            }
            crate::config::BlockedResponse::NxDomain => {
                response.set_response_code(ResponseCode::NXDomain);
            }
            crate::config::BlockedResponse::Ip(ip) => {
                response.set_response_code(ResponseCode::NoError);

                // Add answer with blocked IP
                if let Some(query_q) = query.queries().first() {
                    let mut record = Record::new();
                    record.set_name(query_q.name().clone());
                    record.set_record_type(RecordType::A);
                    record.set_ttl(60);

                    match ip {
                        IpAddr::V4(ipv4) => {
                            record.set_data(Some(RData::A(ipv4.to_owned().into())));
                        }
                        IpAddr::V6(ipv6) => {
                            record.set_data(Some(RData::AAAA(ipv6.to_owned().into())));
                        }
                    }

                    response.add_answer(record);
                }
            }
        }

        response
    }

    /// Forward query to upstream DNS server
    async fn forward_to_upstream(&self, query: Message) -> Result<Message> {
        let upstream = self
            .config
            .server
            .upstream_dns
            .first()
            .ok_or_else(|| anyhow::anyhow!("No upstream DNS configured"))?;

        // Save original query ID
        let original_id = query.id();

        // Forward query
        let query_name = query
            .queries()
            .first()
            .ok_or_else(|| anyhow::anyhow!("No query in message"))?;

        let name = Name::from_str(&query_name.name().to_utf8())?;
        let query_type = query_name.query_type();

        let mut client = self.upstream_client(upstream).await?;
        let dns_response = match client
            .query(name.clone(), hickory_proto::rr::DNSClass::IN, query_type)
            .await
        {
            Ok(response) => response,
            Err(e) => {
                // The cached connection may have gone stale (e.g. the upstream
                // closed an idle HTTP/2 session); reconnect and retry once
                tracing::debug!(error = %e, upstream = %upstream, "Upstream query failed, reconnecting");
                self.upstream_client.lock().await.take();
                let mut client = self.upstream_client(upstream).await?;
                client
                    .query(name, hickory_proto::rr::DNSClass::IN, query_type)
                    .await?
            }
        };

        // Convert DnsResponse to Message and restore original ID
        let mut response: Message = dns_response.into();
        response.set_id(original_id);

        Ok(response)
    }

    /// Get the cached upstream client, connecting if necessary
    async fn upstream_client(&self, upstream: &Upstream) -> Result<AsyncClient> {
        let mut cached = self.upstream_client.lock().await;
        if let Some(client) = cached.as_ref() {
            return Ok(client.clone());
        }
        let client = Self::connect_upstream(upstream).await?;
        *cached = Some(client.clone());
        Ok(client)
    }

    /// Establish a connection to an upstream resolver
    async fn connect_upstream(upstream: &Upstream) -> Result<AsyncClient> {
        let client = match upstream {
            Upstream::Udp(addr) => {
                let stream = UdpClientStream::<UdpSocket>::new(*addr);
                let (client, bg) = AsyncClient::connect(stream).await?;
                tokio::spawn(bg);
                client
            }
            Upstream::DoH { addr, dns_name } => {
                let builder = HttpsClientStreamBuilder::with_client_config(doh_client_config());
                let connect =
                    builder.build::<AsyncIoTokioAsStd<TokioTcpStream>>(*addr, dns_name.clone());
                let (client, bg) = AsyncClient::connect(connect).await?;
                tokio::spawn(bg);
                client
            }
        };
        Ok(client)
    }

    /// Get current statistics
    pub async fn get_stats(&self) -> Statistics {
        let stats = self.stats.read().await;
        Statistics {
            total_queries: stats.total_queries,
            blocked_queries: stats.blocked_queries,
            allowed_queries: stats.allowed_queries,
            start_time: stats.start_time,
        }
    }

    /// Stop the DNS server
    pub async fn stop(&self) -> Result<()> {
        tracing::info!("DNS server stopping...");
        // Server will stop when the run_server loop exits
        Ok(())
    }
}

/// TLS configuration for DoH upstreams, built once (root store parsing isn't free)
fn doh_client_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            roots.add_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.iter().map(|ta| {
                rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
                    ta.subject,
                    ta.spki,
                    ta.name_constraints,
                )
            }));
            Arc::new(
                rustls::ClientConfig::builder()
                    .with_safe_defaults()
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            )
        })
        .clone()
}

// Implement Clone for DnsServer to spawn tasks
impl Clone for DnsServer {
    fn clone(&self) -> Self {
        DnsServer {
            config: Arc::clone(&self.config),
            blocklist: Arc::clone(&self.blocklist),
            stats: Arc::clone(&self.stats),
            upstream_client: Arc::clone(&self.upstream_client),
        }
    }
}

impl Clone for Statistics {
    fn clone(&self) -> Self {
        Statistics {
            total_queries: self.total_queries,
            blocked_queries: self.blocked_queries,
            allowed_queries: self.allowed_queries,
            start_time: self.start_time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Requires network access; run with `cargo test -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn test_doh_upstream_query() {
        let upstream: Upstream = "https://9.9.9.9/dns-query".parse().unwrap();
        let mut client = DnsServer::connect_upstream(&upstream).await.unwrap();

        let name = Name::from_str("example.com.").unwrap();
        let response = client
            .query(name, hickory_proto::rr::DNSClass::IN, RecordType::A)
            .await
            .unwrap();

        assert_eq!(response.response_code(), ResponseCode::NoError);
        assert!(!response.answers().is_empty());
    }
}
