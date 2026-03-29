use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use divine_atbridge::provision_runtime::DbProvisioningKeyStore;
use divine_atbridge::provisioner::KeyStore;
use divine_bridge_db::get_provisioning_key;

const TEST_KEY: [u8; 32] = [
    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
    0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb,
    0xcc, 0xdd, 0xee, 0xff,
];

fn test_database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://divine:divine_dev@[::1]:5432/divine_bridge".to_string())
}

fn execute_batch(conn: &mut PgConnection, sql: &str) {
    for statement in sql
        .split(';')
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        diesel::sql_query(statement).execute(conn).unwrap();
    }
}

fn reset_provisioning_keys_table(database_url: &str) {
    let mut conn =
        PgConnection::establish(database_url).expect("test database should be reachable");
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/004_provisioning_keys/down.sql"),
    );
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/004_provisioning_keys/up.sql"),
    );
}

#[tokio::test]
async fn db_provisioning_key_store_round_trips_generated_keypairs() {
    let database_url = test_database_url();
    reset_provisioning_keys_table(&database_url);

    let store = DbProvisioningKeyStore::new(database_url.clone(), TEST_KEY);
    let (key_ref, generated) = store
        .generate_keypair("plc-rotation-key")
        .await
        .expect("key generation should persist");

    let loaded = store
        .load_keypair(&key_ref)
        .await
        .expect("stored key should decrypt")
        .expect("stored key should exist");

    assert_eq!(
        loaded.secret_key.secret_bytes(),
        generated.secret_key.secret_bytes()
    );
    assert_eq!(loaded.public_key, generated.public_key);

    let mut conn =
        PgConnection::establish(&database_url).expect("test database should be reachable");
    let row = get_provisioning_key(&mut conn, &key_ref)
        .expect("persisted row should load")
        .expect("persisted row should exist");
    assert_eq!(row.key_purpose, "plc-rotation-key");
    assert_ne!(
        row.encrypted_secret.as_slice(),
        generated.secret_key.secret_bytes().as_slice(),
        "secret material must not be stored in plaintext"
    );
}
