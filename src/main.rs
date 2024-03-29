use axum::{extract::Extension, handler::Handler, middleware, routing::get, Router};
use chrono::Local;
use clap::{crate_name, crate_version, App, Arg};
use env_logger::{Builder, Target};
use log::LevelFilter;
use std::io::Write;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;

mod error;
mod handlers;
mod https;
mod metrics;
mod state;

use crate::metrics::{setup_metrics_recorder, track_metrics};
use handlers::{handler_404, health, help, metrics, root};
use https::create_https_client;
use state::State;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let opts = App::new(crate_name!())
        .version(crate_version!())
        .author("")
        .about(crate_name!())
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .help("Set port to listen on")
                .env("ATLAS_BILLING_EXPORTER_LISTEN_PORT")
                .default_value("8080")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("timeout")
                .short("t")
                .long("timeout")
                .help("Set default global timeout")
                .default_value("60")
                .env("ATLAS_BILLING_EXPORTER_TIMEOUT")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("public_key")
                .short("k")
                .long("public_key")
                .help("Set MongoDB Atlas Public Key")
                .required(true)
                .env("ATLAS_BILLING_EXPORTER_PUBLIC_KEY")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("private_key")
                .short("s")
                .long("private_key")
                .help("Set MongoDB Atlas Private Key")
                .required(true)
                .env("ATLAS_BILLING_EXPORTER_PRIVATE_KEY")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("org")
                .short("o")
                .long("org")
                .help("Set org id")
                .required(true)
                .env("ATLAS_BILLING_EXPORTER_ORG_ID")
                .takes_value(true),
        )
        .get_matches();

    // Initialize log Builder
    Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "{{\"date\": \"{}\", \"level\": \"{}\", \"log\": {}}}",
                Local::now().format("%Y-%m-%dT%H:%M:%S:%f"),
                record.level(),
                record.args()
            )
        })
        .target(Target::Stdout)
        .filter_level(LevelFilter::Info)
        .parse_default_env()
        .init();

    // Set port
    let port: u16 = opts.value_of("port").unwrap().parse().unwrap_or_else(|_| {
        eprintln!("specified port isn't in a valid range, setting to 8080");
        8080
    });

    // Create state for axum
    let state = State::new(opts.clone()).await?;

    // Create prometheus handle
    let recorder_handle = setup_metrics_recorder();

    // These should be authenticated
    let base = Router::new().route("/", get(root));

    // These should NOT be authenticated
    let standard = Router::new()
        .route("/health", get(health))
        .route("/help", get(help))
        .route("/metrics", get(metrics));

    let app = Router::new()
        .merge(base)
        .merge(standard)
        .layer(TraceLayer::new_for_http())
        .route_layer(middleware::from_fn(track_metrics))
        .layer(Extension(state))
        .layer(Extension(recorder_handle));

    // add a fallback service for handling routes to unknown paths
    let app = app.fallback(handler_404.into_service());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("Listening on {addr}");
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
