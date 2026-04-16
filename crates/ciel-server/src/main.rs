// Ciel API Server — Binary Entry Point
// See spec Section 1.3 (process boundary) and Section 16.2 (container orchestration).
//
// Initializes the verdict pipeline and starts both REST (port 8080) and gRPC (port 50051)
// servers on the shared tokio runtime.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tracing_subscriber::EnvFilter;

use ciel_checkers::all_stub_checkers;
use ciel_fork::{ForkSimulator, StalenessConfig, StalenessTracker};
use ciel_pipeline::{spawn_outcome_capture, OutcomeCaptureConfig, PipelineConfig, VerdictPipeline};
use ciel_server::grpc::grpc_service;
use ciel_server::metrics::setup_metrics;
use ciel_server::rest::rest_router;
use ciel_server::AppState;
use ciel_signer::CielSigner;

// ---------------------------------------------------------------------------
// Database initialization (Unit 08)
// ---------------------------------------------------------------------------

/// Attempt to connect to PostgreSQL and run migrations.
/// Returns `Some(pool)` on success, `None` on failure (with warning logged).
/// See spec Section 13.2 and docs/08-verdict-store.md.
async fn init_database() -> Option<PgPool> {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            tracing::warn!(
                "DATABASE_URL not set — verdict logging disabled. \
                 See spec Section 13.2."
            );
            return None;
        }
    };

    let pool = match tokio::time::timeout(
        Duration::from_secs(10),
        PgPool::connect(&database_url),
    )
    .await
    {
        Ok(Ok(pool)) => {
            tracing::info!("database connected");
            pool
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "failed to connect to PostgreSQL — verdict logging disabled");
            return None;
        }
        Err(_) => {
            tracing::warn!("PostgreSQL connection timed out after 10s — verdict logging disabled");
            return None;
        }
    };

    match sqlx::migrate!("../../migrations").run(&pool).await {
        Ok(_) => {
            tracing::info!("database migrations applied");
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to run migrations — verdict logging disabled");
            return None;
        }
    }

    Some(pool)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // 1. Initialize structured JSON logging. See spec Section 15.2.
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("ciel-server starting");

    // 2. Setup Prometheus metrics recorder.
    let metrics_handle = setup_metrics();

    // 3. Read port configuration from environment.
    let rest_port: u16 = std::env::var("CIEL_REST_PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse()
        .expect("CIEL_REST_PORT must be a valid port number");
    let grpc_port: u16 = std::env::var("CIEL_GRPC_PORT")
        .unwrap_or_else(|_| "50051".into())
        .parse()
        .expect("CIEL_GRPC_PORT must be a valid port number");

    // 4. Build ForkSimulator. Falls back to offline mode if RPC env vars are missing.
    let fork = match ForkSimulator::new().await {
        Ok(f) => {
            tracing::info!("ForkSimulator connected to RPC");
            f
        }
        Err(e) => {
            tracing::warn!(error = %e, "RPC env vars not set; using offline ForkSimulator (dev mode)");
            ForkSimulator::new_offline()
        }
    };

    // 5. Build StalenessTracker.
    //    In dev mode (offline fork sim), use relaxed thresholds and spawn a
    //    background keepalive task since there's no Geyser subscriber to call
    //    record_update(). In production the Geyser subscriber (Unit 08) does this.
    let is_offline = std::env::var("HELIUS_API_KEY").is_err();
    let staleness_config = if is_offline {
        StalenessConfig {
            warn_threshold: std::time::Duration::from_secs(3600),
            timeout_threshold: std::time::Duration::from_secs(7200),
        }
    } else {
        StalenessConfig::default()
    };
    let staleness = StalenessTracker::new(staleness_config);
    staleness.record_update(0);

    // 6. Build CielSigner from key file or dev fallback.
    let signer = match std::env::var("CIEL_SIGNING_KEY_PATH") {
        Ok(path) => {
            let key_data =
                std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
            let keypair_bytes: Vec<u8> = serde_json::from_slice(&key_data)
                .unwrap_or_else(|e| panic!("invalid JSON keypair at {path}: {e}"));
            let keypair_array: [u8; 64] = keypair_bytes
                .try_into()
                .expect("keypair must be exactly 64 bytes");
            CielSigner::from_keypair_bytes(&keypair_array)
                .unwrap_or_else(|e| panic!("invalid keypair at {path}: {e}"))
        }
        Err(_) => {
            tracing::warn!("CIEL_SIGNING_KEY_PATH not set; using dev signing key");
            CielSigner::from_bytes(&[42u8; 32])
        }
    };

    // 7. Connect to PostgreSQL (graceful degradation). See spec Section 13.1.
    let db_pool = init_database().await;

    // 8. Build stub checkers and verdict pipeline.
    let pipeline = VerdictPipeline::new(
        fork,
        all_stub_checkers(),
        signer,
        staleness,
        db_pool.clone(),
        PipelineConfig::default(),
    );

    // 9. Spawn outcome capture background task (Unit 09). See spec Section 13.3.
    if let Some(ref pool) = db_pool {
        let rpc_url = std::env::var("HELIUS_API_KEY")
            .map(|key| format!("https://mainnet.helius-rpc.com/?api-key={key}"))
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
        spawn_outcome_capture(pool.clone(), rpc_url, OutcomeCaptureConfig::default());
        tracing::info!("outcome capture background task started");
    }

    // 10. Build shared application state.
    let state = Arc::new(AppState {
        pipeline: Arc::new(pipeline),
        metrics_handle,
    });

    // 11. Start REST and gRPC servers concurrently.
    let rest_addr: SocketAddr = ([0, 0, 0, 0], rest_port).into();
    let grpc_addr: SocketAddr = ([0, 0, 0, 0], grpc_port).into();

    tokio::join!(serve_rest(state.clone(), rest_addr), serve_grpc(state, grpc_addr));
}

async fn serve_rest(state: Arc<AppState>, addr: SocketAddr) {
    let router = rest_router(state);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind REST on {addr}: {e}"));
    tracing::info!(%addr, "REST server listening");
    axum::serve(listener, router)
        .await
        .unwrap_or_else(|e| tracing::error!(error = %e, "REST server exited"));
}

async fn serve_grpc(state: Arc<AppState>, addr: SocketAddr) {
    let svc = grpc_service(state);
    tracing::info!(%addr, "gRPC server listening");
    tonic::transport::Server::builder()
        .add_service(svc)
        .serve(addr)
        .await
        .unwrap_or_else(|e| tracing::error!(error = %e, "gRPC server exited"));
}
