use super::{
    refresh_all_chatgpt_auth_tokens, resolve_plan_type, resolve_plan_type_raw,
    resolve_refresh_target, set_current_auth_account_id,
};
use codexmanager_core::auth::parse_id_token_claims;
use codexmanager_core::storage::{now_ts, Account, Storage, Token};

/// 函数 `jwt_with_claims`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - payload: 参数 payload
///
/// # 返回
/// 返回函数执行结果
fn jwt_with_claims(payload: &str) -> String {
    format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig")
}

/// 函数 `build_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - access_token: 参数 access_token
/// - id_token: 参数 id_token
///
/// # 返回
/// 返回函数执行结果
fn build_token(access_token: &str, id_token: &str) -> Token {
    Token {
        account_id: "acc-1".to_string(),
        id_token: id_token.to_string(),
        access_token: access_token.to_string(),
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: 0,
    }
}

fn build_account(
    id: &str,
    chatgpt_account_id: Option<&str>,
    workspace_id: Option<&str>,
) -> Account {
    Account {
        id: id.to_string(),
        label: id.to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: chatgpt_account_id.map(str::to_string),
        workspace_id: workspace_id.map(str::to_string),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    }
}

fn build_refresh_token(account_id: &str, refresh_token: &str) -> Token {
    Token {
        account_id: account_id.to_string(),
        id_token: "id".to_string(),
        access_token: "access".to_string(),
        refresh_token: refresh_token.to_string(),
        api_key_access_token: None,
        last_refresh: 0,
    }
}

const AUTH_TEST_DB_PATH_ENV: &str = "CODEXMANAGER_DB_PATH";

struct AuthTestDatabase {
    path: std::path::PathBuf,
    storage: Option<Storage>,
}

impl AuthTestDatabase {
    fn new(label: &str) -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let sequence = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut database = Self {
            path: std::env::temp_dir().join(format!(
                "codexmanager-auth-{label}-{}-{timestamp}-{sequence}.sqlite",
                std::process::id()
            )),
            storage: None,
        };
        // Finish migrations while the path is still private to this fixture.
        let storage = Storage::open(&database.path).expect("open storage");
        storage.init().expect("init storage");
        database.storage = Some(storage);
        database
    }

    fn storage(&self) -> &Storage {
        self.storage.as_ref().expect("initialized fixture storage")
    }
}

impl Drop for AuthTestDatabase {
    fn drop(&mut self) {
        drop(self.storage.take());
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.path.as_os_str().to_os_string();
            path.push(suffix);
            let _ = std::fs::remove_file(std::path::PathBuf::from(path));
        }
    }
}

struct DbPathEnvGuard {
    previous: Option<std::ffi::OsString>,
}

impl DbPathEnvGuard {
    fn publish(database: &AuthTestDatabase) -> Self {
        let previous = std::env::var_os(AUTH_TEST_DB_PATH_ENV);
        std::env::set_var(AUTH_TEST_DB_PATH_ENV, &database.path);
        Self { previous }
    }
}

impl Drop for DbPathEnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.as_ref() {
            std::env::set_var(AUTH_TEST_DB_PATH_ENV, previous);
        } else {
            std::env::remove_var(AUTH_TEST_DB_PATH_ENV);
        }
    }
}

#[test]
fn auth_database_initializes_before_publication_and_restores_path_after_unwind() {
    let _guard = crate::test_env_guard();
    let previous = std::env::var_os(AUTH_TEST_DB_PATH_ENV);
    let database = AuthTestDatabase::new("unwind");
    assert!(
        std::env::var_os(AUTH_TEST_DB_PATH_ENV) == previous,
        "private initialization must not publish the fixture path"
    );
    assert!(database
        .storage()
        .find_account_by_id("fixture-probe")
        .expect("account schema is ready before publication")
        .is_none());

    let db_path = DbPathEnvGuard::publish(&database);
    assert!(std::env::var_os(AUTH_TEST_DB_PATH_ENV).as_deref() == Some(database.path.as_os_str()));
    let outcome = std::panic::catch_unwind(move || {
        let _db_path = db_path;
        panic!("injected auth fixture failure after publication");
    });
    assert!(outcome.is_err(), "injected failure must unwind");
    assert!(
        std::env::var_os(AUTH_TEST_DB_PATH_ENV) == previous,
        "unwinding must restore the original database path"
    );
}

#[test]
fn resolve_refresh_target_prefers_explicit_account_id_over_current_account() {
    let _guard = crate::test_env_guard();
    let database = AuthTestDatabase::new("target");
    let _db_path = DbPathEnvGuard::publish(&database);

    {
        let storage = database.storage();
        storage
            .insert_account(&build_account("acc-current", Some("org-current"), None))
            .expect("insert current account");
        storage
            .insert_account(&build_account(
                "acc-target",
                Some("org-target"),
                Some("ws-target"),
            ))
            .expect("insert target account");
        storage
            .insert_token(&build_refresh_token("acc-current", "refresh-current"))
            .expect("insert current token");
        storage
            .insert_token(&build_refresh_token("acc-target", "refresh-target"))
            .expect("insert target token");
        set_current_auth_account_id(Some("acc-current")).expect("set current account");

        let (account, token) = resolve_refresh_target(&storage, Some("acc-target"))
            .expect("resolve target")
            .expect("target exists");

        assert_eq!(account.id, "acc-target");
        assert_eq!(token.refresh_token, "refresh-target");
        set_current_auth_account_id(None).expect("clear current account");
    }
}

#[test]
fn refresh_all_chatgpt_auth_tokens_skips_accounts_without_refresh_token() {
    let _guard = crate::test_env_guard();
    let database = AuthTestDatabase::new("refresh-all");
    let _db_path = DbPathEnvGuard::publish(&database);

    {
        let storage = database.storage();
        storage
            .insert_account(&build_account("missing-token", None, None))
            .expect("insert missing token account");
        storage
            .insert_account(&build_account("missing-refresh", None, None))
            .expect("insert missing refresh account");
        storage
            .insert_token(&build_refresh_token("missing-refresh", ""))
            .expect("insert empty refresh token");

        let result = refresh_all_chatgpt_auth_tokens().expect("refresh all");

        assert_eq!(result.requested, 0);
        assert_eq!(result.succeeded, 0);
        assert_eq!(result.failed, 0);
        assert_eq!(result.skipped, 2);
        assert_eq!(result.results.len(), 2);
        assert!(result
            .results
            .iter()
            .any(|item| item.account_id == "missing-token"
                && item.message.as_deref() == Some("missing token")));
        assert!(result
            .results
            .iter()
            .any(|item| item.account_id == "missing-refresh"
                && item.message.as_deref() == Some("missing refresh_token")));
    }
}

/// 函数 `resolve_plan_type_prefers_latest_access_token_claims`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn resolve_plan_type_prefers_latest_access_token_claims() {
    let access_token = jwt_with_claims(
        "eyJzdWIiOiJ1c2VyLTEiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9wbGFuX3R5cGUiOiJnbyJ9fQ",
    );
    let id_token = jwt_with_claims(
        "eyJzdWIiOiJ1c2VyLTEiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9wbGFuX3R5cGUiOiJwcm8ifX0",
    );
    let token = build_token(&access_token, &id_token);
    let claims = parse_id_token_claims(&access_token).expect("access claims");

    let resolved = resolve_plan_type(&token, Some(&claims));

    assert_eq!(resolved.as_deref(), Some("go"));
}

/// 函数 `resolve_plan_type_falls_back_to_id_token_when_access_claims_missing`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn resolve_plan_type_falls_back_to_id_token_when_access_claims_missing() {
    let access_token = jwt_with_claims("eyJzdWIiOiJ1c2VyLTEifQ");
    let id_token = jwt_with_claims(
        "eyJzdWIiOiJ1c2VyLTEiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9wbGFuX3R5cGUiOiJ0ZWFtIn19",
    );
    let token = build_token(&access_token, &id_token);
    let claims = parse_id_token_claims(&access_token).expect("access claims");

    let resolved = resolve_plan_type(&token, Some(&claims));

    assert_eq!(resolved.as_deref(), Some("team"));
}

/// 函数 `resolve_plan_type_preserves_unknown_raw_value_for_diagnostics`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn resolve_plan_type_preserves_unknown_raw_value_for_diagnostics() {
    let access_token = jwt_with_claims(
        "eyJzdWIiOiJ1c2VyLTEiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9wbGFuX3R5cGUiOiJyZXNlYXJjaGVyX2JldGEifX0",
    );
    let token = build_token(&access_token, &access_token);
    let claims = parse_id_token_claims(&access_token).expect("access claims");

    let resolved = resolve_plan_type(&token, Some(&claims));
    let raw = resolve_plan_type_raw(&token, Some(&claims));

    assert_eq!(resolved.as_deref(), Some("unknown"));
    assert_eq!(raw.as_deref(), Some("researcher_beta"));
}
