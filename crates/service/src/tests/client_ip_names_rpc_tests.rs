use super::*;

#[test]
fn client_ip_names_rpc_are_private_to_admins() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("client-ip-names-private");
    let names = serde_json::json!({"192.0.2.1": "Example Person"});
    crate::app_settings_set(Some(&serde_json::json!({"clientIpNames": names}))).unwrap();
    let member = RpcActor::from_parts(Some(ROLE_MEMBER), Some("example-member"));
    for actor in [member.clone(), RpcActor::system_admin()] {
        let response = response_result(handle_request_with_actor(
            rpc_request("appSettings/get", serde_json::json!({})),
            actor.clone(),
        ));
        assert!(response.result.get("error").is_none());
        if actor.is_admin() {
            assert_eq!(response.result["clientIpNames"], names);
        } else {
            assert!(response.result.get("clientIpNames").is_none());
        }
    }
    set_web_access_password(Some("password123")).unwrap();
    set_web_auth_mode("password").unwrap();
    let response = response_result(handle_request_with_actor(
        rpc_request("appSettings/set", serde_json::json!({"clientIpNames": {}})),
        member.clone(),
    ));
    assert!(rpc_error(&response).contains("permission_denied"));
    assert_eq!(crate::app_settings_get().unwrap()["clientIpNames"], names);
    let response = response_result(handle_request_with_actor(
        rpc_request(
            "appSettings/set",
            serde_json::json!({"updateAutoCheck": false}),
        ),
        member,
    ));
    assert!(response.result.get("error").is_none());
    assert!(response.result.get("clientIpNames").is_none());
    let _ = std::fs::remove_file(db_path);
}
