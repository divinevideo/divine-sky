# Jetstream Slice

This slice exposes a Jetstream instance against the localnet PDS firehose.

## Intended Hostname

Expose the TLS proxy on `jetstream.<tailnet>.ts.net`.

## Defaults

`env.example` points `JETSTREAM_WS_URL` at the localnet PDS subscription endpoint:

`wss://pds.<tailnet>.ts.net/xrpc/com.atproto.sync.subscribeRepos`

Replace the Tailscale placeholders before bringing the slice up.
