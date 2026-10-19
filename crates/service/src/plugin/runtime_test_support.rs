//! Test-only ownership of the plugin client cache and local fixture tasks.
use std::sync::MutexGuard;
use std::time::Duration;

pub(crate) struct PluginHttpClientGuard<'lock> {
    previous: Option<reqwest::Client>,
    _lock: &'lock MutexGuard<'static, ()>,
}

impl<'lock> PluginHttpClientGuard<'lock> {
    pub(crate) fn enter(lock: &'lock MutexGuard<'static, ()>) -> Self {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(20))
            .build()
            .expect("build local plugin fixture client");
        Self::replace(lock, Some(client))
    }

    fn replace(lock: &'lock MutexGuard<'static, ()>, client: Option<reqwest::Client>) -> Self {
        let cache = super::PLUGIN_HTTP_CLIENT.get_or_init(|| std::sync::Mutex::new(None));
        let previous = std::mem::replace(
            &mut *crate::lock_utils::lock_recover(cache, "plugin_http_client_test"),
            client,
        );
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for PluginHttpClientGuard<'_> {
    fn drop(&mut self) {
        let cache = super::PLUGIN_HTTP_CLIENT.get_or_init(|| std::sync::Mutex::new(None));
        *crate::lock_utils::lock_recover(cache, "plugin_http_client_test") = self.previous.take();
    }
}

pub(crate) struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> AbortOnDrop<T> {
    pub(crate) fn new(task: tokio::task::JoinHandle<T>) -> Self {
        Self(task)
    }

    pub(crate) async fn abort_and_join(&mut self) -> Result<(), String> {
        self.0.abort();
        match tokio::time::timeout(Duration::from_secs(2), &mut self.0).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) if error.is_cancelled() => Ok(()),
            Ok(Err(error)) => Err(format!("local plugin fixture server failed: {error}")),
            Err(_) => Err("local plugin fixture server did not stop within 2s".to_string()),
        }
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(crate) fn finish_fixture<T>(outcome: std::thread::Result<T>, cleanup: Result<(), String>) -> T {
    match outcome {
        Ok(value) => {
            cleanup.expect("bounded plugin fixture cleanup");
            value
        }
        Err(panic) => {
            if let Err(error) = cleanup {
                eprintln!("plugin fixture cleanup also failed: {error}");
            }
            std::panic::resume_unwind(panic);
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn local_plugin_client_restores_polluted_default_cache_after_panic() {
    use futures_util::FutureExt;
    use std::ffi::OsString;
    use std::panic::AssertUnwindSafe;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    if crate::loopback_proxy_test::run_proxy_control_in_isolated_process_async().await {
        return;
    }

    #[derive(Default)]
    struct EnvironmentRestore(Vec<(&'static str, Option<OsString>)>);

    impl EnvironmentRestore {
        fn change(&mut self, key: &'static str, value: Option<&str>) {
            self.0.push((key, std::env::var_os(key)));
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }

    impl Drop for EnvironmentRestore {
        fn drop(&mut self) {
            // Windows aliases uppercase and lowercase environment names.
            while let Some((key, value)) = self.0.pop() {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    async fn bounded_fetch(url: &str) -> String {
        tokio::time::timeout(Duration::from_secs(2), super::fetch_text(url))
            .await
            .expect("bounded plugin fixture request")
            .expect("plugin fixture response")
    }

    let lock = crate::test_env_guard();
    let _original_cache = PluginHttpClientGuard::replace(&lock, None);
    let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_url = format!("http://{}/fixture", upstream.local_addr().unwrap());
    let proxy_hits = Arc::new(AtomicUsize::new(0));
    let direct_hits = Arc::new(AtomicUsize::new(0));
    let proxy_app = axum::Router::new().fallback({
        let hits = proxy_hits.clone();
        move || {
            hits.fetch_add(1, Ordering::SeqCst);
            async { "proxy-body" }
        }
    });
    let direct_app = axum::Router::new().fallback({
        let hits = direct_hits.clone();
        move || {
            hits.fetch_add(1, Ordering::SeqCst);
            async { "direct-body" }
        }
    });
    let mut proxy_server = AbortOnDrop::new(tokio::spawn(async move {
        axum::serve(proxy, proxy_app).await.unwrap();
    }));
    let mut direct_server = AbortOnDrop::new(tokio::spawn(async move {
        axum::serve(upstream, direct_app).await.unwrap();
    }));
    let outcome = AssertUnwindSafe(async {
        let mut environment = EnvironmentRestore::default();
        for key in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            environment.change(key, Some(&proxy_url));
        }
        environment.change("NO_PROXY", Some("unrelated.invalid"));
        environment.change("no_proxy", Some("unrelated.invalid"));
        environment.change("REQUEST_METHOD", None);

        // Initialize the real default cache while a controlled host proxy is active.
        assert_eq!(bounded_fetch(&upstream_url).await, "proxy-body");
        assert_eq!(proxy_hits.load(Ordering::SeqCst), 1);
        assert_eq!(direct_hits.load(Ordering::SeqCst), 0);
        let builds = super::plugin_http_client_build_count_for_test();

        let panic = AssertUnwindSafe(async {
            let _local_client = PluginHttpClientGuard::enter(&lock);
            assert_eq!(bounded_fetch(&upstream_url).await, "direct-body");
            assert_eq!(proxy_hits.load(Ordering::SeqCst), 1);
            assert_eq!(direct_hits.load(Ordering::SeqCst), 1);
            panic!("injected local plugin fixture panic");
        })
        .catch_unwind()
        .await
        .expect_err("injected fixture panic must be caught");
        assert_eq!(
            panic.downcast_ref::<&str>(),
            Some(&"injected local plugin fixture panic")
        );

        // The same cached production client, rather than a rebuilt replacement,
        // must use its original proxy again after the local guard unwinds.
        assert_eq!(bounded_fetch(&upstream_url).await, "proxy-body");
        assert_eq!(proxy_hits.load(Ordering::SeqCst), 2);
        assert_eq!(direct_hits.load(Ordering::SeqCst), 1);
        assert_eq!(super::plugin_http_client_build_count_for_test(), builds);
    })
    .catch_unwind()
    .await;
    let (proxy_cleanup, direct_cleanup) = tokio::join!(
        proxy_server.abort_and_join(),
        direct_server.abort_and_join()
    );
    finish_fixture(outcome, proxy_cleanup.and(direct_cleanup));
}
