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
