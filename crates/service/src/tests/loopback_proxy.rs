//! Local HTTP fixtures opt into this guard while holding the existing lib-test
//! environment lock. Production proxy selection and other tests stay unchanged.
use std::ffi::OsString;
use std::io::Write;
use std::sync::MutexGuard;

const LOOPBACK_NO_PROXY: &str = "127.0.0.1,localhost,::1";

/// Proxy negative controls deliberately redirect every default HTTP client.
/// A process-local environment lock cannot protect clients that do not use it,
/// or undo the proxy matcher captured by their process-lifetime caches.
pub(crate) async fn run_proxy_control_in_isolated_process_async() -> bool {
    use std::process::Stdio;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const CHILD_TEST: &str = "CODEXMANAGER_PROXY_CONTROL_CHILD_TEST";
    let current = std::thread::current();
    let test_name = current.name().expect("named proxy control test").to_owned();
    if std::env::var(CHILD_TEST).as_deref() == Ok(test_name.as_str()) {
        return false;
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "codexmanager-proxy-control-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).expect("create proxy control output directory");
    let stdout_path = directory.join("stdout.log");
    let stderr_path = directory.join("stderr.log");
    let spawned = {
        // Only snapshot the inherited environment under the existing lock.
        // The child has its own lock, environment and cached clients.
        let _environment = crate::test_env_guard();
        tokio::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test_name, "--nocapture"])
            .env(CHILD_TEST, &test_name)
            .stdout(Stdio::from(std::fs::File::create(&stdout_path).unwrap()))
            .stderr(Stdio::from(std::fs::File::create(&stderr_path).unwrap()))
            .kill_on_drop(true)
            .spawn()
    };
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            let cleanup = std::fs::remove_dir_all(&directory);
            panic!("start proxy control {test_name}: {error}; cleanup: {cleanup:?}");
        }
    };
    let pid = child.id().expect("proxy child ID");
    eprintln!("isolated proxy control started: test={test_name} pid={pid}");
    let waited = tokio::time::timeout(Duration::from_secs(20), child.wait()).await;
    let (status, timed_out, termination_error) = match waited {
        Ok(status) => (status, false, None),
        Err(_) => {
            let kill_error = child.start_kill().err().map(|error| error.to_string());
            let reaped = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
            let status = reaped.unwrap_or_else(|_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "proxy control did not exit within 2s after termination",
                ))
            });
            (status, true, kill_error)
        }
    };
    let stdout = std::fs::read_to_string(&stdout_path);
    let stderr = std::fs::read_to_string(&stderr_path);
    let cleanup = std::fs::remove_dir_all(&directory);
    eprintln!(
        "isolated proxy control finished: test={test_name} pid={pid} timed_out={timed_out} termination_error={termination_error:?} status={status:?} cleanup={cleanup:?}"
    );
    if let Ok(output) = &stdout {
        print!("{output}");
    }
    if let Ok(output) = &stderr {
        eprint!("{output}");
    }
    assert!(
        !timed_out && termination_error.is_none() && matches!(status, Ok(status) if status.success())
            && stdout.is_ok() && stderr.is_ok() && cleanup.is_ok(),
        "isolated proxy control failed: {test_name}; status={status:?}; timed_out={timed_out}; termination_error={termination_error:?}; cleanup={cleanup:?}"
    );
    true
}

pub(crate) fn run_proxy_control_in_isolated_process() -> bool {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("proxy control supervisor runtime")
        .block_on(run_proxy_control_in_isolated_process_async())
}

#[derive(Default)]
struct EnvironmentRestore(Vec<(&'static str, Option<OsString>)>);

impl EnvironmentRestore {
    fn change(&mut self, name: &'static str, value: Option<&str>) {
        self.0.push((name, std::env::var_os(name)));
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    fn restore(&mut self) {
        // Windows aliases uppercase/lowercase names. Restore the last mutation
        // first, including changes made before construction can finish.
        while let Some((name, original)) = self.0.pop() {
            match original {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

impl Drop for EnvironmentRestore {
    fn drop(&mut self) {
        self.restore();
    }
}

pub(crate) struct LoopbackProxyGuard<'lock> {
    environment: EnvironmentRestore,
    reload: fn(),
    // Borrowing the original lock prevents callers from releasing it before
    // the environment and clients have both been restored.
    _lock: &'lock MutexGuard<'static, ()>,
}

impl<'lock> LoopbackProxyGuard<'lock> {
    pub(crate) fn enter(lock: &'lock MutexGuard<'static, ()>) -> Self {
        Self::enter_with_reload(lock, crate::gateway::reload_runtime_config_from_env)
    }

    fn enter_with_reload(lock: &'lock MutexGuard<'static, ()>, reload: fn()) -> Self {
        let before = loopback_intercepted();
        let mut guard = Self {
            environment: EnvironmentRestore::default(),
            reload,
            _lock: lock,
        };
        guard
            .environment
            .change("NO_PROXY", Some(LOOPBACK_NO_PROXY));
        guard
            .environment
            .change("no_proxy", Some(LOOPBACK_NO_PROXY));
        guard
            .environment
            .change("CODEXMANAGER_UPSTREAM_PROXY_URL", None);
        guard.environment.change("CODEXMANAGER_PROXY_LIST", None);
        // The dependency checks presence, so assigning an empty value would
        // still enable CGI handling of HTTP_PROXY.
        guard.environment.change("REQUEST_METHOD", None);
        let after = loopback_intercepted();
        eprintln!("lib fixture loopback proxy intercept: before={before} after={after}");
        assert!(!after, "local fixture must bypass inherited system proxy");
        // Existing cached clients already contain their proxy Matcher. Refresh
        // them only after applying the scoped environment.
        (guard.reload)();
        guard
    }
}

impl Drop for LoopbackProxyGuard<'_> {
    fn drop(&mut self) {
        self.environment.restore();
        let result = std::panic::catch_unwind(self.reload);
        if let Err(error) = result {
            if std::thread::panicking() {
                // Do not abort the process with a second panic during unwind.
                // Keep the original test failure and expose this cleanup failure
                // without printing proxy values or a panic payload.
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "lib fixture proxy cache restoration panicked during unwind"
                );
            } else {
                std::panic::resume_unwind(error);
            }
        }
    }
}

fn loopback_intercepted() -> bool {
    let destination = "http://127.0.0.1:1/".parse().expect("loopback URI");
    hyper_util::client::proxy::matcher::Matcher::from_system()
        .intercept(&destination)
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::time::Duration;

    const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
    const PROXY_NAMES: [&str; 11] = [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
        "CODEXMANAGER_UPSTREAM_PROXY_URL",
        "CODEXMANAGER_PROXY_LIST",
        "REQUEST_METHOD",
    ];

    // Outer controls also restore their environment before refreshing clients,
    // including when an assertion in a test fails.
    struct ControlledEnvironment<'lock> {
        environment: EnvironmentRestore,
        _lock: &'lock MutexGuard<'static, ()>,
    }

    impl<'lock> ControlledEnvironment<'lock> {
        fn enter(lock: &'lock MutexGuard<'static, ()>, proxy_url: &str) -> Self {
            let mut guard = Self {
                environment: EnvironmentRestore::default(),
                _lock: lock,
            };
            for &name in &PROXY_NAMES[..6] {
                guard.environment.change(name, Some(proxy_url));
            }
            // A nonempty nonmatching list prevents host ProxyOverride from
            // supplying a bypass rule that would invalidate the negative control.
            for name in ["NO_PROXY", "no_proxy"] {
                guard.environment.change(name, Some("unrelated.invalid"));
            }
            for name in ["CODEXMANAGER_UPSTREAM_PROXY_URL", "CODEXMANAGER_PROXY_LIST"] {
                guard.environment.change(name, Some(proxy_url));
            }
            guard.environment.change("REQUEST_METHOD", None);
            crate::gateway::reload_runtime_config_from_env();
            guard
        }
    }

    impl Drop for ControlledEnvironment<'_> {
        fn drop(&mut self) {
            self.environment.restore();
            let result = catch_unwind(crate::gateway::reload_runtime_config_from_env);
            if let Err(error) = result {
                if std::thread::panicking() {
                    let _ = writeln!(
                        std::io::stderr().lock(),
                        "controlled proxy cache restoration panicked during unwind"
                    );
                } else {
                    std::panic::resume_unwind(error);
                }
            }
        }
    }

    fn snapshot() -> Vec<(&'static str, Option<OsString>)> {
        PROXY_NAMES
            .iter()
            .map(|&name| (name, std::env::var_os(name)))
            .collect()
    }

    fn assert_environment(expected: &[(&str, Option<OsString>)]) {
        for (name, original) in expected {
            assert!(
                std::env::var_os(name) == *original,
                "proxy environment {name} must be restored"
            );
        }
    }

    fn bind_listener() -> TcpListener {
        let listener = TcpListener::bind("127.0.0.1:0").expect("local fixture listener");
        listener.set_nonblocking(true).unwrap();
        listener
    }

    fn assert_no_connection(listener: &TcpListener) {
        assert!(matches!(
            listener.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ));
    }

    fn send(client: reqwest::Client, url: &str) -> Result<String, reqwest::Error> {
        crate::process_runtime().unwrap().block_on(async {
            client
                .get(url)
                .timeout(PROBE_TIMEOUT)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await
        })
    }

    fn assert_direct(client: reqwest::Client, listener: &TcpListener, url: &str) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        crate::process_runtime().unwrap().block_on(async {
            let listener = tokio::net::TcpListener::from_std(listener.try_clone().unwrap()).unwrap();
            tokio::time::timeout(PROBE_TIMEOUT, async {
                let (response, ()) = tokio::join!(
                    async {
                        let response = client.get(url).timeout(PROBE_TIMEOUT).send().await.unwrap();
                        assert_eq!(response.status(), reqwest::StatusCode::OK);
                        response.text().await.unwrap()
                    },
                    async {
                        let (mut stream, _) = listener.accept().await.unwrap();
                        let mut headers = Vec::new();
                        while !headers.windows(4).any(|part| part == b"\r\n\r\n") {
                            let mut buffer = [0; 512];
                            let count = stream.read(&mut buffer).await.unwrap();
                            assert!(count > 0, "direct request has complete HTTP headers");
                            headers.extend_from_slice(&buffer[..count]);
                            assert!(headers.len() < 8192, "bounded local request headers");
                        }
                        stream
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\nConnection: close\r\n\r\nloopback direct")
                            .await
                            .unwrap();
                        stream.shutdown().await.unwrap();
                    }
                );
                assert_eq!(response, "loopback direct");
            })
            .await
            .expect("bounded direct local HTTP probe");
        });
    }

    #[test]
    fn loopback_proxy_guard_bypasses_host_proxy_and_preserves_explicit_proxy_and_cache() {
        if run_proxy_control_in_isolated_process() {
            return;
        }
        let lock = crate::test_env_guard();
        let original = snapshot();
        let proxy = bind_listener();
        let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
        let controlled = ControlledEnvironment::enter(&lock, &proxy_url);
        let controlled_environment = snapshot();
        let upstream = bind_listener();
        let url = format!("http://{}/fixture", upstream.local_addr().unwrap());
        assert!(loopback_intercepted());
        // Initialize the same default cache used by candidate execution before
        // entering the guard. Empty account IDs avoid consulting an account DB.
        let _cached_before = crate::gateway::async_upstream_client_for_account("").unwrap();

        let negative = send(reqwest::Client::new(), &url).unwrap_err();
        assert!(negative.is_timeout());
        drop(proxy.accept().expect("default client reaches silent proxy"));
        assert_no_connection(&upstream);

        let loopback = LoopbackProxyGuard::enter(&lock);
        assert!(!loopback_intercepted());
        assert!(std::env::var_os("REQUEST_METHOD").is_none());
        assert!(std::env::var_os("CODEXMANAGER_UPSTREAM_PROXY_URL").is_none());
        assert!(std::env::var_os("CODEXMANAGER_PROXY_LIST").is_none());
        assert_direct(reqwest::Client::new(), &upstream, &url);
        assert_direct(
            crate::gateway::async_upstream_client_for_account("").unwrap(),
            &upstream,
            &url,
        );
        assert_no_connection(&proxy);

        let explicit = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(&proxy_url).unwrap())
            .build()
            .unwrap();
        assert!(send(explicit, &url).unwrap_err().is_timeout());
        drop(proxy.accept().expect("explicit proxy stays authoritative"));
        assert_no_connection(&upstream);

        drop(loopback);
        assert_environment(&controlled_environment);
        assert!(loopback_intercepted());
        assert!(
            crate::gateway::current_upstream_proxy_url().as_deref() == Some(proxy_url.as_str())
        );
        let restored = crate::gateway::async_upstream_client_for_account("").unwrap();
        assert!(send(restored, &url).unwrap_err().is_timeout());
        drop(
            proxy
                .accept()
                .expect("restored cached client uses proxy again"),
        );
        assert_no_connection(&upstream);
        drop(controlled);
        assert_environment(&original);
    }

    #[test]
    fn loopback_proxy_guard_restores_environment_and_runtime_after_panic() {
        if run_proxy_control_in_isolated_process() {
            return;
        }
        let lock = crate::test_env_guard();
        let original = snapshot();
        let mut controlled = ControlledEnvironment::enter(&lock, "http://127.0.0.1:9");
        controlled.environment.change("REQUEST_METHOD", Some("GET"));
        let controlled_environment = snapshot();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _loopback = LoopbackProxyGuard::enter(&lock);
            assert!(crate::gateway::current_upstream_proxy_url().is_none());
            assert!(std::env::var_os("REQUEST_METHOD").is_none());
            panic!("injected local fixture panic");
        }));
        assert!(result.is_err());
        assert_environment(&controlled_environment);
        assert!(
            crate::gateway::current_upstream_proxy_url().as_deref() == Some("http://127.0.0.1:9")
        );
        drop(controlled);
        assert_environment(&original);
    }

    #[test]
    fn loopback_proxy_guard_restores_environment_if_cache_reload_panics_during_entry() {
        if run_proxy_control_in_isolated_process() {
            return;
        }
        fn fail_reload() {
            panic!("injected cache reload failure");
        }

        let lock = crate::test_env_guard();
        let original = snapshot();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _loopback = LoopbackProxyGuard::enter_with_reload(&lock, fail_reload);
        }));
        assert!(result.is_err());
        // The callback also panics during Drop. Reaching this assertion proves
        // restoration did not abort while handling the original entry panic.
        assert_environment(&original);
    }
}
