# DNS Slice

This slice holds the local `divine.test` DNS surface for the ATProto lab.

## What It Contains

- `coredns` for wildcard `A` and `_atproto` TXT handling under `*.divine.test`
- `app` reserved for the local handle-admin service
- `tailscale` for tailnet exposure
- `nginx` for the local admin HTTPS entrypoint

## Intended Hostname

Expose the admin endpoint on `handles.<tailnet>.ts.net`.

## Notes

- `app` now runs the `divine-localnet-admin` crate and writes `db.divine.test` into the shared `/zones` volume that CoreDNS reads.
- Create mappings through the local admin URL with `POST /api/handles` and a payload like `{"name":"alice","did":"did:plc:alice123"}`.
- Each mapping produces `alice.divine.test` plus `_atproto.alice.divine.test TXT "did=did:plc:alice123"` in the generated zone file.
