use super::test_support::{finish_fixture, AbortOnDrop, PluginHttpClientGuard};
use super::{plugin_http_client, plugin_http_client_build_count_for_test};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;

#[test]
fn plugin_http_client_reuses_cached_client() {
    let lock = crate::test_env_guard();
    let _client = PluginHttpClientGuard::enter(&lock);
    let before = plugin_http_client_build_count_for_test();
    let first = plugin_http_client().expect("first plugin http client");
    let after_first = plugin_http_client_build_count_for_test();
    let second = plugin_http_client().expect("second plugin http client");
    let after_second = plugin_http_client_build_count_for_test();

    assert!(
        after_first == before || after_first == before + 1,
        "expected first call to reuse an existing client or build one client, before={before}, after_first={after_first}"
    );
    assert_eq!(after_second, after_first);
    drop(first);
    drop(second);
}

#[tokio::test(flavor = "current_thread")]
async fn plugin_async_network_waits_leave_executor_and_blocking_workers_available() {
    let lock = crate::test_env_guard();
    let _client = PluginHttpClientGuard::enter(&lock);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
    let (entered, mut started) = tokio::sync::mpsc::channel(32);
    let app = axum::Router::new()
        .route(
            "/slow",
            axum::routing::get({
                let gate = gate.clone();
                move || {
                    let gate = gate.clone();
                    let entered = entered.clone();
                    async move {
                        entered.send(()).await.unwrap();
                        gate.acquire().await.unwrap().forget();
                        "plugin-body"
                    }
                }
            }),
        )
        .route("/fast", axum::routing::get(|| async { "fast-body" }))
        .route(
            "/error",
            axum::routing::get(|| async { axum::http::StatusCode::BAD_GATEWAY }),
        );
    let mut server = AbortOnDrop::new(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let mut requests = tokio::task::JoinSet::new();
    let outcome = AssertUnwindSafe(async {
        let request_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
        for index in 0..32 {
            let url = format!("{base}/slow");
            requests.spawn(async move { (index, super::fetch_text(&url).await) });
        }
        let mut entered_count = 0;
        let startup = tokio::time::timeout_at(request_deadline, async {
            while entered_count < 32 {
                tokio::select! {
                    signal = started.recv() => {
                        assert!(signal.is_some(), "local plugin entry channel closed after {entered_count}/32 requests");
                        entered_count += 1;
                    }
                    completed = requests.join_next() => {
                        panic!("plugin request completed before release barrier after {entered_count}/32 entries: {completed:?}");
                    }
                }
            }
        })
        .await;
        assert!(
            startup.is_ok(),
            "only {entered_count}/32 plugin requests reached the local release barrier within 20s"
        );
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                super::fetch_text(&format!("{base}/fast"))
            )
            .await
            .unwrap()
            .unwrap(),
            "fast-body"
        );
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                tokio::task::spawn_blocking(|| 17)
            )
            .await
            .expect("blocking worker remains available during plugin network waits")
            .unwrap(),
            17
        );
        gate.add_permits(32);
        let mut completed_count = 0;
        tokio::time::timeout_at(request_deadline, async {
            while let Some(request) = requests.join_next().await {
                let (index, response) = request.expect("join plugin fixture request");
                assert_eq!(
                    response.unwrap_or_else(|error| panic!(
                        "plugin fixture request {index} failed: {error}"
                    )),
                    "plugin-body"
                );
                completed_count += 1;
            }
        })
        .await
        .expect("all 32 plugin fixture requests finish after release");
        assert_eq!(completed_count, 32);
        assert!(super::fetch_text(&format!("{base}/error"))
            .await
            .unwrap_err()
            .contains("502"));
    })
    .catch_unwind()
    .await;
    // Release any accepted handlers before cancelling the client requests.
    // JoinSet's Drop remains a fallback; the ordinary and panic paths both
    // explicitly observe every client task and the server finishing here.
    gate.add_permits(32);
    requests.abort_all();
    let request_cleanup = async {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while requests.join_next().await.is_some() {}
        })
        .await
        .map_err(|_| "plugin fixture requests did not stop within 2s".to_string())
    };
    let (request_cleanup, server_cleanup) = tokio::join!(request_cleanup, server.abort_and_join());
    finish_fixture(outcome, request_cleanup.and(server_cleanup));
}

#[tokio::test(flavor = "current_thread")]
async fn rhai_network_host_uses_shared_async_client_and_enforces_permission() {
    let lock = crate::test_env_guard();
    let _client = PluginHttpClientGuard::enter(&lock);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = axum::Router::new().fallback({
        let calls = calls.clone();
        move |method: axum::http::Method, body: String| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move { format!("{method}:{body}") }
        }
    });
    let mut server = AbortOnDrop::new(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let outcome = AssertUnwindSafe(async {
        let script = format!(
            r#"fn run(context) {{
        let first = http_get("{base}/get");
        let second = http_post("{base}/post", "payload");
        #{{ first: first["body"], second: second["body"] }}
    }}"#
        );
        let result = tokio::task::spawn_blocking(move || {
            let plugin = codexmanager_core::storage::PluginRuntimeInstall {
                plugin_id: "async-network".to_string(),
                source_url: None,
                name: "Async network".to_string(),
                version: "1".to_string(),
                script_body: script,
                permissions_json: "[\"network\"]".to_string(),
                status: "enabled".to_string(),
            };
            let task = codexmanager_core::storage::PluginTaskExecutionRow {
                id: "async-network::run".to_string(),
                plugin_id: plugin.plugin_id.clone(),
                name: "Run".to_string(),
                description: None,
                entrypoint: "run".to_string(),
                schedule_kind: "manual".to_string(),
                interval_seconds: None,
                enabled: true,
            };
            let granted = super::parse_permissions(&plugin.permissions_json);
            let result = super::execute_plugin_script(&plugin, &task, None, &granted, 1).unwrap();
            assert!(
                super::execute_plugin_script(&plugin, &task, None, &Default::default(), 1).is_err()
            );
            result
        })
        .await
        .unwrap();
        assert_eq!(result["first"], "GET:");
        assert_eq!(result["second"], "POST:payload");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    })
    .catch_unwind()
    .await;
    let cleanup = server.abort_and_join().await;
    finish_fixture(outcome, cleanup);
}
