//! Integration test: JWT-based tenant resolution with a real OIDC provider.
//!
//! Spins up [FerrisKey](https://github.com/ferriskey/ferriskey) (an open-source
//! IAM server) as a Docker container via **testcontainers**, obtains a real JWT
//! through the standard OIDC password grant, and verifies that the
//! `JwtTenantResolver` middleware correctly extracts the tenant from the token.
//!
//! Requires Docker to be running.

use axum::routing::get;
use axum::Router;
use serde_json::Value;
use std::time::Duration;
use tenant_axum::config::{HttpTenantConfig, HttpTenantStrategy};
use tenant_axum::{CurrentTenant, TenantLayer};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use testcontainers_modules::postgres::Postgres;
use tokio::net::TcpListener;

/// Create a unique Docker network for inter-container communication.
async fn create_docker_network(name: &str) {
    let _ = tokio::process::Command::new("docker")
        .args(["network", "rm", name])
        .output()
        .await;
    let output = tokio::process::Command::new("docker")
        .args(["network", "create", name])
        .output()
        .await
        .expect("failed to create Docker network");
    assert!(
        output.status.success(),
        "docker network create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Remove a Docker network.
async fn remove_docker_network(name: &str) {
    let _ = tokio::process::Command::new("docker")
        .args(["network", "rm", name])
        .output()
        .await;
}

/// Start a Postgres container configured for FerrisKey on the given network.
async fn start_ferriskey_postgres(network: &str) -> (ContainerAsync<Postgres>, u16) {
    let container = Postgres::default()
        .with_db_name("ferriskey")
        .with_user("ferriskey")
        .with_password("ferriskey")
        .with_tag("16-alpine")
        .with_network(network)
        .start()
        .await
        .unwrap();

    let host_port = container.get_host_port_ipv4(5432).await.unwrap();
    (container, host_port)
}

/// Get the container's IP address on the given custom network.
async fn container_ip_on_network(container_id: &str, network: &str) -> String {
    let tmpl = format!(
        "{{{{(index .NetworkSettings.Networks \"{}\").IPAddress}}}}",
        network
    );
    let output = tokio::process::Command::new("docker")
        .args(["inspect", "-f", &tmpl, container_id])
        .output()
        .await
        .expect("failed to inspect container");
    let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(
        !ip.is_empty(),
        "container {} has no IP on network {}",
        container_id,
        network
    );
    ip
}

/// Run FerrisKey database migrations via `docker run --rm`.
///
/// Uses the same `ferriskey-api` image which bundles the `sqlx` CLI and the
/// migration files.  The container connects to Postgres via the network IP so
/// both containers communicate over the custom Docker network.
async fn run_ferriskey_migrations(pg_ip: &str, network: &str) {
    let db_url = format!("postgresql://ferriskey:ferriskey@{}:5432/ferriskey", pg_ip);

    // Retry a few times in case Postgres is not yet accepting connections.
    let mut last_output = None;
    for attempt in 0..5 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let output = tokio::process::Command::new("docker")
            .args([
                "run",
                "--rm",
                "--network",
                network,
                "-e",
                &format!("DATABASE_URL={}", db_url),
                "--entrypoint",
                "sqlx",
                "ghcr.io/ferriskey/ferriskey-api:latest",
                "migrate",
                "run",
                "--source",
                "/usr/local/src/ferriskey/migrations",
            ])
            .output()
            .await
            .expect("failed to run migration container");

        if output.status.success() {
            return;
        }
        last_output = Some(output);
    }

    let output = last_output.unwrap();
    panic!(
        "FerrisKey migrations failed after retries.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Start the FerrisKey API container on the given network.
///
/// The API listens on port 3333 (configurable via `SERVER_PORT`).
/// We wait for the `"listening on"` log line on stderr before returning.
async fn start_ferriskey_api(pg_ip: &str, network: &str) -> (ContainerAsync<GenericImage>, u16) {
    let container = GenericImage::new("ghcr.io/ferriskey/ferriskey-api", "latest")
        .with_exposed_port(3333.tcp())
        .with_wait_for(WaitFor::message_on_stderr("listening on"))
        // ImageExt methods ↓
        .with_env_var("DATABASE_HOST", pg_ip)
        .with_env_var("DATABASE_PORT", "5432")
        .with_env_var("DATABASE_NAME", "ferriskey")
        .with_env_var("DATABASE_USER", "ferriskey")
        .with_env_var("DATABASE_PASSWORD", "ferriskey")
        .with_env_var("DATABASE_SCHEMA", "public")
        .with_env_var("SERVER_PORT", "3333")
        .with_env_var("SERVER_HOST", "0.0.0.0")
        .with_env_var("ALLOWED_ORIGINS", "http://localhost")
        .with_env_var("ADMIN_USERNAME", "admin")
        .with_env_var("ADMIN_PASSWORD", "admin")
        .with_env_var("ADMIN_EMAIL", "admin@local")
        .with_network(network)
        .with_startup_timeout(Duration::from_secs(120))
        .start()
        .await
        .unwrap();

    let port = container.get_host_port_ipv4(3333).await.unwrap();
    (container, port)
}

/// Start an Axum app in the background and return its base URL.
async fn serve(app: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    format!("http://127.0.0.1:{}", port)
}

/// Full round-trip: FerrisKey issues a real JWT → our middleware resolves the
/// tenant from the `azp` claim (authorized party / client_id).
#[tokio::test]
async fn jwt_tenant_resolved_from_ferriskey_token() {
    let network = "fk-test-net";
    create_docker_network(network).await;

    let (pg_container, _pg_host_port) = start_ferriskey_postgres(network).await;
    let pg_id = pg_container.id().to_string();
    let pg_ip = container_ip_on_network(&pg_id, network).await;

    run_ferriskey_migrations(&pg_ip, network).await;

    let (_fk_container, fk_port) = start_ferriskey_api(&pg_ip, network).await;
    let fk_base = format!("http://127.0.0.1:{}", fk_port);

    let http = reqwest::Client::new();

    let admin_token: String = {
        let resp = http
            .post(format!(
                "{}/realms/master/protocol/openid-connect/token",
                fk_base
            ))
            .form(&[
                ("grant_type", "password"),
                ("client_id", "admin-cli"),
                ("username", "admin"),
                ("password", "admin"),
            ])
            .send()
            .await
            .expect("admin token request failed");
        assert!(
            resp.status().is_success(),
            "admin token request returned {}",
            resp.status()
        );
        let body: Value = resp.json().await.unwrap();
        body["access_token"]
            .as_str()
            .expect("no access_token in admin response")
            .to_string()
    };

    let realm_resp = http
        .post(format!("{}/realms", fk_base))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({ "name": "acme" }))
        .send()
        .await
        .unwrap();
    assert!(
        realm_resp.status().is_success(),
        "create realm: {} – {}",
        realm_resp.status(),
        realm_resp.text().await.unwrap()
    );

    let client_resp = http
        .post(format!("{}/realms/acme/clients", fk_base))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({
            "name": "Tenant App",
            "client_id": "tenant-app",
            "client_type": "public",
            "protocol": "openid-connect",
            "enabled": true,
            "direct_access_grants_enabled": true,
            "public_client": true,
            "service_account_enabled": false
        }))
        .send()
        .await
        .unwrap();
    assert!(
        client_resp.status().is_success(),
        "create client: {} – {}",
        client_resp.status(),
        client_resp.text().await.unwrap()
    );

    let user_resp: Value = http
        .post(format!("{}/realms/acme/users", fk_base))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({
            "username": "alice",
            "firstname": "Alice",
            "lastname": "Smith",
            "email": "alice@acme.example",
            "email_verified": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let user_id = user_resp["data"]["id"]
        .as_str()
        .expect("no id in create-user response");

    let pw_resp = http
        .put(format!(
            "{}/realms/acme/users/{}/reset-password",
            fk_base, user_id
        ))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({
            "temporary": false,
            "credential_type": "password",
            "value": "alice-pass"
        }))
        .send()
        .await
        .unwrap();
    assert!(
        pw_resp.status().is_success(),
        "reset password: {} – {}",
        pw_resp.status(),
        pw_resp.text().await.unwrap()
    );

    let user_token_resp: Value = http
        .post(format!(
            "{}/realms/acme/protocol/openid-connect/token",
            fk_base
        ))
        .form(&[
            ("grant_type", "password"),
            ("client_id", "tenant-app"),
            ("username", "alice"),
            ("password", "alice-pass"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let user_jwt = user_token_resp["access_token"]
        .as_str()
        .expect("no access_token in user token response");

    async fn handler(CurrentTenant(tid): CurrentTenant) -> String {
        tid.as_ref().to_string()
    }

    let app = Router::new()
        .route("/whoami", get(handler))
        .layer(TenantLayer::from_config(
            &HttpTenantConfig::builder()
                .strategy(HttpTenantStrategy::Jwt)
                .jwt_claim_name("azp")
                .build(),
        ));

    let app_base = serve(app).await;

    let resp = http
        .get(format!("{}/whoami", app_base))
        .header("Authorization", format!("Bearer {}", user_jwt))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status().as_u16(), 200);
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "tenant-app",
        "expected tenant from azp claim to be 'tenant-app'"
    );

    drop(_fk_container);
    drop(pg_container);
    remove_docker_network(network).await;
}

/// Verify that a request without a JWT still returns 400 (missing tenant).
#[tokio::test]
async fn jwt_missing_bearer_returns_400() {
    async fn handler(CurrentTenant(tid): CurrentTenant) -> String {
        tid.as_ref().to_string()
    }

    let app = Router::new()
        .route("/whoami", get(handler))
        .layer(TenantLayer::from_config(
            &HttpTenantConfig::builder()
                .strategy(HttpTenantStrategy::Jwt)
                .jwt_claim_name("azp")
                .build(),
        ));

    let app_base = serve(app).await;

    let resp = reqwest::get(format!("{}/whoami", app_base)).await.unwrap();

    assert_eq!(resp.status().as_u16(), 400, "expected 400 for missing JWT");
}
