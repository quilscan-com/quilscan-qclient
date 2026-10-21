use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use num_bigint::BigInt;
use quil_rpc::NodeRpcServer;
use quil_types::consensus::{ShardDetail, ShardInfoProvider};
use quil_types::proto::node::node_service_server::NodeServiceServer;
use tonic::codegen::{http, BoxFuture, Service};

use super::{actions, fetch_once_snapshot, Client};

struct Shards;

impl ShardInfoProvider for Shards {
    fn get_shard_info(
        &self,
        include_all: bool,
    ) -> quil_types::error::Result<(Vec<ShardDetail>, u64, BigInt, u64, BigInt)> {
        assert!(include_all);
        Ok((Vec::new(), 1, BigInt::from(1), 42, BigInt::from(0)))
    }
}

// Delay the actual HTTP/2 request future asynchronously, so a slow test RPC
// cannot block the runtime responsible for the client's deadline timer.
#[derive(Clone)]
struct DelayedReads<S> {
    inner: S,
    worker_delay: Duration,
    shard_delay: Duration,
}

impl<S: tonic::server::NamedService> tonic::server::NamedService for DelayedReads<S> {
    const NAME: &'static str = S::NAME;
}

impl<S, B> Service<http::Request<B>> for DelayedReads<S>
where
    S: Service<http::Request<B>>,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<Self::Response, Self::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: http::Request<B>) -> Self::Future {
        let delay = if request.uri().path().ends_with("/GetWorkerInfo") {
            self.worker_delay
        } else if request.uri().path().ends_with("/GetShardInfo") {
            self.shard_delay
        } else {
            Duration::ZERO
        };
        let response = self.inner.call(request);
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            response.await
        })
    }
}

struct TestServer {
    client: Client,
    multiaddr: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(
    node_delay: Duration,
    worker_delay: Duration,
    shard_delay: Option<Duration>,
) -> TestServer {
    let mut server = NodeRpcServer::new()
        .with_peer_id("delayed-node".into())
        .with_peer_score_provider(Arc::new(move || {
            Box::pin(async move {
                tokio::time::sleep(node_delay).await;
                1.0
            })
        }));
    if shard_delay.is_some() {
        server = server.with_shard_info_provider(Arc::new(Shards));
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let endpoint = format!("http://{addr}");
    let incoming = futures::stream::unfold(listener, |listener| async {
        let socket = listener.accept().await.map(|(socket, _)| socket);
        Some((socket, listener))
    });
    let task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(DelayedReads {
                inner: NodeServiceServer::new(server),
                worker_delay,
                shard_delay: shard_delay.unwrap_or_default(),
            })
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });
    let channel = tonic::transport::Endpoint::from_shared(endpoint)
        .unwrap()
        .connect()
        .await
        .unwrap();
    let client = Client::new(channel);
    TestServer {
        client,
        multiaddr: format!("/ip4/127.0.0.1/tcp/{}", addr.port()),
        task,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn once_snapshot_accepts_node_info_after_fast_deadline() {
    let server = serve(
        Duration::from_millis(150),
        Duration::ZERO,
        Some(Duration::ZERO),
    )
    .await;
    let fast = fetch_once_snapshot(
        server.client.clone(),
        Duration::from_millis(40),
        actions::RPC_TIMEOUT,
        Duration::from_millis(500),
    )
    .await
    .unwrap_err();
    assert!(fast.to_string().contains("GetNodeInfo: timed out"));
    let result = fetch_once_snapshot(
        server.client.clone(),
        Duration::from_millis(500),
        actions::RPC_TIMEOUT,
        Duration::from_millis(500),
    )
    .await
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["peer_id"], "delayed-node");
    assert_eq!(json["worker_info_available"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn once_snapshot_accepts_shards_after_fast_deadline() {
    let server = serve(
        Duration::ZERO,
        Duration::ZERO,
        Some(Duration::from_millis(150)),
    )
    .await;
    let fast = fetch_once_snapshot(
        server.client.clone(),
        Duration::from_millis(500),
        actions::RPC_TIMEOUT,
        Duration::from_millis(40),
    )
    .await
    .unwrap_err();
    assert!(fast
        .to_string()
        .contains("GetShardInfo: timed out after 40ms"));
    let result = fetch_once_snapshot(
        server.client.clone(),
        Duration::from_millis(500),
        actions::RPC_TIMEOUT,
        Duration::from_millis(500),
    )
    .await
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["peer_id"], "delayed-node");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn once_snapshot_returns_bounded_errors_for_slow_node_or_shards() {
    for (node_delay, shard_delay, stage) in [
        (Duration::from_millis(500), Duration::ZERO, "GetNodeInfo"),
        (Duration::ZERO, Duration::from_millis(500), "GetShardInfo"),
    ] {
        let server = serve(node_delay, Duration::ZERO, Some(shard_delay)).await;
        let started = Instant::now();
        let error = fetch_once_snapshot(
            server.client.clone(),
            Duration::from_millis(40),
            actions::RPC_TIMEOUT,
            Duration::from_millis(40),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(
            started.elapsed() < Duration::from_millis(300),
            "unbounded {stage} read"
        );
        assert!(error.contains(stage), "{error}");
        assert!(error.contains("timed out"), "{error}");
        assert!(error.contains("one-shot deadlines"), "{error}");
        assert!(!error.contains("retrying"), "{error}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn once_snapshot_keeps_worker_timeout_optional_and_independent() {
    let server = serve(
        Duration::ZERO,
        Duration::from_millis(150),
        Some(Duration::ZERO),
    )
    .await;
    for (worker_timeout, expected_available) in [
        (Duration::from_millis(40), false),
        (Duration::from_millis(500), true),
    ] {
        let result = fetch_once_snapshot(
            server.client.clone(),
            Duration::from_millis(500),
            worker_timeout,
            Duration::from_millis(500),
        )
        .await
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(json["worker_info_available"], expected_available);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn once_snapshot_preserves_rpc_errors_instead_of_fabricating_success() {
    let server = serve(Duration::ZERO, Duration::ZERO, None).await;
    let error = fetch_once_snapshot(
        server.client.clone(),
        Duration::from_millis(500),
        actions::RPC_TIMEOUT,
        Duration::from_millis(500),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("fetch shard data"), "{error}");
    assert!(error.contains("shard info not available"), "{error}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn run_once_uses_the_snapshot_fetch_path_without_terminal_initialization() {
    let server = serve(Duration::ZERO, Duration::ZERO, Some(Duration::ZERO)).await;
    let directory = tempfile::tempdir().unwrap();
    let pc = super::ProverCtx {
        node_config: quil_config::Config::default(),
        connect_opts: crate::rpc::ConnectOpts {
            listen_grpc_multiaddr: server.multiaddr.clone(),
            ..Default::default()
        },
        key_manager: Arc::new(
            quil_keys::FileKeyManager::new(
                directory.path().join("keys.yml"),
                &hex::encode([54; 32]),
                "q-prover-key".into(),
                Box::new(quil_crypto::FalconKeyConstructor),
            )
            .unwrap(),
        ),
    };
    super::run_once(&pc).await.unwrap();
}

#[test]
fn once_production_read_budget_fits_existing_agent_deadline() {
    assert_eq!(actions::RPC_TIMEOUT, Duration::from_secs(15));
    assert_eq!(actions::SHARD_RPC_TIMEOUT, Duration::from_secs(60));
    assert_eq!(super::ONCE_NODE_RPC_TIMEOUT, Duration::from_secs(75));
    assert_eq!(super::ONCE_SHARD_RPC_TIMEOUT, Duration::from_secs(90));
    let reads =
        (super::ONCE_NODE_RPC_TIMEOUT + actions::RPC_TIMEOUT).max(super::ONCE_SHARD_RPC_TIMEOUT);
    assert_eq!(reads, Duration::from_secs(90));
    assert!(Duration::from_secs(20) + reads < Duration::from_secs(120));
}
