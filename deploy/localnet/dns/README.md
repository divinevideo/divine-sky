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

- `Corefile.example` shows the wildcard `A` responses, `_atproto.<name>.divine.test` TXT pattern, and a local admin hostname on the tailnet.
- The `app` service is a placeholder here and will be replaced by the Rust handle-admin crate in the next task.
