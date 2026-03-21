# PDS Slice

This slice runs a Tailscale-exposed ATProto PDS for the localnet lab.

## Intended Hostname

Expose the TLS proxy on `pds.<tailnet>.ts.net`, and configure handles under `*.divine.test`.

## Required Placeholders

Set the values in `env.example` before bringing the slice up:

- `PDS_HOSTNAME`
- `PDS_SERVICE_DID`
- `PDS_SERVICE_HANDLE_DOMAINS=.divine.test`
- `PDS_ADMIN_PASSWORD`
- `PDS_DID_PLC_URL`
- `PDS_IMAGE`

## Notes

- `PDS_IMAGE` defaults to `ghcr.io/blacksky-algorithms/rsky-pds:latest`, but the compose file keeps it overrideable for compatibility testing.
- The slice reuses `config/minio-init.sh` so bucket creation stays consistent with the rest of the repo.
