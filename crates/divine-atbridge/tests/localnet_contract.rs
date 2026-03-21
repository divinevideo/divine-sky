fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap()
        .to_path_buf()
}

#[test]
fn localnet_docs_and_layout_are_present() {
    let repo_root = repo_root();
    assert!(repo_root.join("deploy/localnet/README.md").exists());
    let bootstrap =
        std::fs::read_to_string(repo_root.join("docs/runbooks/dev-bootstrap.md")).unwrap();
    assert!(bootstrap.contains("deploy/localnet"));
}

#[test]
fn localnet_plc_and_pds_compose_files_define_required_services() {
    let repo_root = repo_root();
    let plc = std::fs::read_to_string(repo_root.join("deploy/localnet/plc/docker-compose.yml"))
        .unwrap();
    let pds = std::fs::read_to_string(repo_root.join("deploy/localnet/pds/docker-compose.yml"))
        .unwrap();
    assert!(plc.contains("tailscale:"));
    assert!(plc.contains("app:"));
    assert!(pds.contains("PDS_DID_PLC_URL"));
    assert!(pds.contains("PDS_IMAGE"));
}

#[test]
fn localnet_jetstream_and_dns_slices_are_defined() {
    let repo_root = repo_root();
    let jetstream =
        std::fs::read_to_string(repo_root.join("deploy/localnet/jetstream/docker-compose.yml"))
            .unwrap();
    let dns = std::fs::read_to_string(repo_root.join("deploy/localnet/dns/docker-compose.yml"))
        .unwrap();
    assert!(jetstream.contains("JETSTREAM_WS_URL"));
    assert!(dns.contains("coredns:"));
    assert!(dns.contains("app:"));
}

#[test]
fn localnet_override_examples_target_local_services() {
    let repo_root = repo_root();
    let bridge_env =
        std::fs::read_to_string(repo_root.join("deploy/localnet/bridge.env.example")).unwrap();
    let gateway_env = std::fs::read_to_string(
        repo_root.join("deploy/localnet/handle-gateway.env.example"),
    )
    .unwrap();
    assert!(bridge_env.contains("PLC_DIRECTORY_URL=https://plc."));
    assert!(bridge_env.contains("HANDLE_DOMAIN=divine.test"));
    assert!(gateway_env.contains("ATPROTO_PROVISIONING_URL"));
}

#[test]
fn localnet_scripts_and_runbooks_exist() {
    let repo_root = repo_root();
    assert!(repo_root.join("scripts/localnet-up.sh").exists());
    assert!(repo_root.join("docs/runbooks/localnet-lab.md").exists());
}
