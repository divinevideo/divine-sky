# ATProto Auth Server Smoke Test

## Purpose

Verify that a `ready` DiVine account can be used by an external Bluesky-compatible client through the delegated ATProto Authorization Server on `entryway.divine.video`.

This smoke test covers the live ATProto launch contract:

- PDS protected-resource discovery
- Authorization Server metadata discovery
- PAR
- browser authorization with an existing DiVine login
- authorization-code token exchange
- refresh-token rotation
- authenticated PDS access with the returned access token

Current limitations to account for during testing:

- access-token trust is implemented in `rsky-pds`
- DPoP nonces and replay tracking are stored in-process today, so retries must use the latest `DPoP-Nonce` returned by the responding server instance
- disabling or unlinking a user blocks new approvals immediately, but existing access tokens remain valid until expiry

## Preconditions

- The test user already has:
  - a claimed `*.divine.video` username
  - `atproto_enabled = true`
  - `atproto_state = "ready"`
  - a non-null `atproto_did`
- Keycast is configured with:
  - `APP_URL=https://login.divine.video`
  - `ATPROTO_OAUTH_JWT_PRIVATE_KEY_HEX`
  - `ATPROTO_OAUTH_PDS_DID` or `PDS_SERVICE_DID`
- `rsky-pds` is configured with:
  - `PDS_SERVICE_DID`
  - `PDS_ENTRYWAY_URL=https://entryway.divine.video`
  - `PDS_ENTRYWAY_JWT_PUBLIC_KEY_HEX` matching the public key for keycast `ATPROTO_OAUTH_JWT_PRIVATE_KEY_HEX`

## 1. Discover The PDS Protected Resource

Run:

```bash
curl -sS https://pds.divine.video/.well-known/oauth-protected-resource | jq
```

Expect:

- `resource` points at the PDS public URL
- `authorization_servers` is exactly `["https://entryway.divine.video"]`

## 2. Discover The Authorization Server

Run:

```bash
curl -sS https://entryway.divine.video/.well-known/oauth-authorization-server | jq
```

Expect at least:

- `issuer = "https://entryway.divine.video"`
- `authorization_endpoint = "https://entryway.divine.video/api/oauth/authorize"`
- `token_endpoint = "https://entryway.divine.video/api/oauth/token"`
- `pushed_authorization_request_endpoint = "https://entryway.divine.video/api/oauth/par"`
- `token_endpoint_auth_methods_supported` includes `none`
- `require_pushed_authorization_requests = true`

This smoke test uses the public-client path only.

- Public client: send `client_id` directly and do not include `client_assertion`.

## 3. Create A PAR Request

Generate a PKCE verifier and challenge:

```bash
CODE_VERIFIER="$(openssl rand -base64 48 | tr '+/' '-_' | tr -d '=')"
CODE_CHALLENGE="$(printf '%s' "$CODE_VERIFIER" | openssl dgst -binary -sha256 | openssl base64 -A | tr '+/' '-_' | tr -d '=')"
```

Submit PAR:

```bash
P256_DPOP_PRIV_HEX="$(openssl rand -hex 32)"
P256_DPOP_JWK="$(ruby -ropenssl -rbase64 -rjson -e '
group = OpenSSL::PKey::EC::Group.new("prime256v1")
key = OpenSSL::PKey::EC.new(group)
key.private_key = OpenSSL::BN.new(ENV.fetch("P256_DPOP_PRIV_HEX"), 16)
key.public_key = group.generator.mul(key.private_key)
point = key.public_key.to_octet_string(:uncompressed)
x = Base64.urlsafe_encode64(point[1,32], padding: false)
y = Base64.urlsafe_encode64(point[33,32], padding: false)
puts JSON.generate({kty: "EC", crv: "P-256", x: x, y: y})
')"
PAR_IAT="$(date +%s)"
PAR_DPOP="$(ruby -ropenssl -rbase64 -rjson -rsecurerandom -e '
header = { typ: "dpop+jwt", alg: "ES256", jwk: JSON.parse(ENV.fetch("P256_DPOP_JWK")) }
payload = {
  jti: "par-#{SecureRandom.uuid}",
  htm: "POST",
  htu: "https://entryway.divine.video/api/oauth/par",
  iat: Integer(ENV.fetch("PAR_IAT"))
}
segments = [
  Base64.urlsafe_encode64(JSON.generate(header), padding: false),
  Base64.urlsafe_encode64(JSON.generate(payload), padding: false),
]
digest = OpenSSL::Digest::SHA256.digest(segments.join("."))
asn1 = OpenSSL::PKey::EC.new(OpenSSL::PKey::EC::Group.new("prime256v1")).tap { |k|
  k.private_key = OpenSSL::BN.new(ENV.fetch("P256_DPOP_PRIV_HEX"), 16)
  k.public_key = k.group.generator.mul(k.private_key)
}.dsa_sign_asn1(digest)
sig = OpenSSL::ASN1.decode(asn1).value.map { |bn| bn.value.to_s(2).rjust(32, "\x00") }.join
puts "#{segments.join(".")}.#{Base64.urlsafe_encode64(sig, padding: false)}"
')"

curl -sS \
  -D /tmp/atproto-par-headers.txt \
  -X POST https://entryway.divine.video/api/oauth/par \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  -H "DPoP: $PAR_DPOP" \
  --data-urlencode 'client_id=https://example-client.invalid' \
  --data-urlencode 'redirect_uri=https://example-client.invalid/callback' \
  --data-urlencode 'scope=atproto' \
  --data-urlencode 'state=smoke-test-state' \
  --data-urlencode "code_challenge=$CODE_CHALLENGE" \
  --data-urlencode 'code_challenge_method=S256' | jq
```

For a confidential-client variant, add `client_assertion_type` and `client_assertion` to the same PAR request.

Expect:

- `request_uri` is returned
- `expires_in` is non-zero
- `/tmp/atproto-par-headers.txt` contains a `DPoP-Nonce` header

Save the nonce for the next token request:

```bash
PAR_NONCE="$(awk 'BEGIN{IGNORECASE=1}/^DPoP-Nonce:/{print $2}' /tmp/atproto-par-headers.txt | tr -d '\r')"
```

## 4. Complete Browser Authorization

Open:

```text
https://entryway.divine.video/api/oauth/authorize?request_uri=<urlencoded request_uri>
```

Expect:

- if not already logged in, the browser is redirected to the normal DiVine login page
- after login, the flow returns to the ATProto authorization step
- if the account is `ready`, approval completes and the browser is redirected to the client callback URL
- the callback query includes:
  - `code`
  - `state=smoke-test-state`
  - `iss=https://entryway.divine.video`

If the account is not `ready`, expect the authorization request to fail instead of issuing a code.

## 5. Exchange The Authorization Code

Run:

```bash
TOKEN_DPOP="$(ruby -ropenssl -rbase64 -rjson -rsecurerandom -e '
header = { typ: "dpop+jwt", alg: "ES256", jwk: JSON.parse(ENV.fetch("P256_DPOP_JWK")) }
payload = {
  jti: "token-#{SecureRandom.uuid}",
  htm: "POST",
  htu: "https://entryway.divine.video/api/oauth/token",
  iat: Integer(`date +%s`),
  nonce: ENV.fetch("PAR_NONCE")
}
segments = [
  Base64.urlsafe_encode64(JSON.generate(header), padding: false),
  Base64.urlsafe_encode64(JSON.generate(payload), padding: false),
]
digest = OpenSSL::Digest::SHA256.digest(segments.join("."))
asn1 = OpenSSL::PKey::EC.new(OpenSSL::PKey::EC::Group.new("prime256v1")).tap { |k|
  k.private_key = OpenSSL::BN.new(ENV.fetch("P256_DPOP_PRIV_HEX"), 16)
  k.public_key = k.group.generator.mul(k.private_key)
}.dsa_sign_asn1(digest)
sig = OpenSSL::ASN1.decode(asn1).value.map { |bn| bn.value.to_s(2).rjust(32, "\x00") }.join
puts "#{segments.join(".")}.#{Base64.urlsafe_encode64(sig, padding: false)}"
')"

curl -sS \
  -D /tmp/atproto-token-headers.txt \
  -X POST https://entryway.divine.video/api/oauth/token \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  -H "DPoP: $TOKEN_DPOP" \
  --data-urlencode 'grant_type=authorization_code' \
  --data-urlencode "code=<code from callback>" \
  --data-urlencode 'client_id=https://example-client.invalid' \
  --data-urlencode 'redirect_uri=https://example-client.invalid/callback' \
  --data-urlencode "code_verifier=$CODE_VERIFIER" | jq
```

Expect:

- `token_type = "DPoP"`
- `scope = "atproto"`
- `sub = <user did:plc>`
- `access_token` is present
- `refresh_token` is present
- `/tmp/atproto-token-headers.txt` contains a rotated `DPoP-Nonce`
- the access token payload includes `cnf.jkt`

Save the rotated nonce:

```bash
TOKEN_NONCE="$(awk 'BEGIN{IGNORECASE=1}/^DPoP-Nonce:/{print $2}' /tmp/atproto-token-headers.txt | tr -d '\r')"
ACCESS_TOKEN="<access_token>"
REFRESH_TOKEN="<refresh_token>"
```

## 6. Rotate The Refresh Token

Run:

```bash
REFRESH_DPOP="$(ruby -ropenssl -rbase64 -rjson -rsecurerandom -e '
header = { typ: "dpop+jwt", alg: "ES256", jwk: JSON.parse(ENV.fetch("P256_DPOP_JWK")) }
payload = {
  jti: "refresh-#{SecureRandom.uuid}",
  htm: "POST",
  htu: "https://entryway.divine.video/api/oauth/token",
  iat: Integer(`date +%s`),
  nonce: ENV.fetch("TOKEN_NONCE")
}
segments = [
  Base64.urlsafe_encode64(JSON.generate(header), padding: false),
  Base64.urlsafe_encode64(JSON.generate(payload), padding: false),
]
digest = OpenSSL::Digest::SHA256.digest(segments.join("."))
asn1 = OpenSSL::PKey::EC.new(OpenSSL::PKey::EC::Group.new("prime256v1")).tap { |k|
  k.private_key = OpenSSL::BN.new(ENV.fetch("P256_DPOP_PRIV_HEX"), 16)
  k.public_key = k.group.generator.mul(k.private_key)
}.dsa_sign_asn1(digest)
sig = OpenSSL::ASN1.decode(asn1).value.map { |bn| bn.value.to_s(2).rjust(32, "\x00") }.join
puts "#{segments.join(".")}.#{Base64.urlsafe_encode64(sig, padding: false)}"
')"

curl -sS \
  -D /tmp/atproto-refresh-headers.txt \
  -X POST https://entryway.divine.video/api/oauth/token \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  -H "DPoP: $REFRESH_DPOP" \
  --data-urlencode 'grant_type=refresh_token' \
  --data-urlencode "refresh_token=$REFRESH_TOKEN" \
  --data-urlencode 'client_id=https://example-client.invalid' | jq
```

Expect:

- a new `access_token`
- a new `refresh_token`
- the returned refresh token differs from the earlier one
- `/tmp/atproto-refresh-headers.txt` contains a new `DPoP-Nonce`

## 7. Call The PDS With The Access Token

Run:

```bash
RESOURCE_ATH="$(ruby -rbase64 -ropenssl -e 'print Base64.urlsafe_encode64(OpenSSL::Digest::SHA256.digest(ENV.fetch("ACCESS_TOKEN")), padding: false)')"
RESOURCE_PROBE_DPOP="$(ruby -ropenssl -rbase64 -rjson -rsecurerandom -e '
header = { typ: "dpop+jwt", alg: "ES256", jwk: JSON.parse(ENV.fetch("P256_DPOP_JWK")) }
payload = {
  jti: "resource-probe-#{SecureRandom.uuid}",
  htm: "GET",
  htu: "https://pds.divine.video/xrpc/com.atproto.server.getSession",
  iat: Integer(`date +%s`),
  ath: ENV.fetch("RESOURCE_ATH")
}
segments = [
  Base64.urlsafe_encode64(JSON.generate(header), padding: false),
  Base64.urlsafe_encode64(JSON.generate(payload), padding: false),
]
digest = OpenSSL::Digest::SHA256.digest(segments.join("."))
asn1 = OpenSSL::PKey::EC.new(OpenSSL::PKey::EC::Group.new("prime256v1")).tap { |k|
  k.private_key = OpenSSL::BN.new(ENV.fetch("P256_DPOP_PRIV_HEX"), 16)
  k.public_key = k.group.generator.mul(k.private_key)
}.dsa_sign_asn1(digest)
sig = OpenSSL::ASN1.decode(asn1).value.map { |bn| bn.value.to_s(2).rjust(32, "\x00") }.join
puts "#{segments.join(".")}.#{Base64.urlsafe_encode64(sig, padding: false)}"
')"
RESOURCE_NONCE="$(curl -sS -D /tmp/atproto-resource-headers.txt \
  https://pds.divine.video/xrpc/com.atproto.server.getSession \
  -H "Authorization: DPoP $ACCESS_TOKEN" \
  -H "DPoP: $RESOURCE_PROBE_DPOP" \
  -o /tmp/atproto-resource-body.json || true; \
  awk 'BEGIN{IGNORECASE=1}/^DPoP-Nonce:/{print $2}' /tmp/atproto-resource-headers.txt | tr -d '\r')"
RESOURCE_DPOP="$(ruby -ropenssl -rbase64 -rjson -rsecurerandom -e '
header = { typ: "dpop+jwt", alg: "ES256", jwk: JSON.parse(ENV.fetch("P256_DPOP_JWK")) }
payload = {
  jti: "resource-#{SecureRandom.uuid}",
  htm: "GET",
  htu: "https://pds.divine.video/xrpc/com.atproto.server.getSession",
  iat: Integer(`date +%s`),
  nonce: ENV.fetch("RESOURCE_NONCE"),
  ath: ENV.fetch("RESOURCE_ATH")
}
segments = [
  Base64.urlsafe_encode64(JSON.generate(header), padding: false),
  Base64.urlsafe_encode64(JSON.generate(payload), padding: false),
]
digest = OpenSSL::Digest::SHA256.digest(segments.join("."))
asn1 = OpenSSL::PKey::EC.new(OpenSSL::PKey::EC::Group.new("prime256v1")).tap { |k|
  k.private_key = OpenSSL::BN.new(ENV.fetch("P256_DPOP_PRIV_HEX"), 16)
  k.public_key = k.group.generator.mul(k.private_key)
}.dsa_sign_asn1(digest)
sig = OpenSSL::ASN1.decode(asn1).value.map { |bn| bn.value.to_s(2).rjust(32, "\x00") }.join
puts "#{segments.join(".")}.#{Base64.urlsafe_encode64(sig, padding: false)}"
')"

curl -sS \
  https://pds.divine.video/xrpc/com.atproto.server.getSession \
  -H "Authorization: DPoP $ACCESS_TOKEN" \
  -H "DPoP: $RESOURCE_DPOP" | jq
```

Expect:

- the first request returns `400` and issues `DPoP-Nonce`
- HTTP `200`
- `did` matches the `sub` returned by the token response
- the returned session belongs to the same DiVine-linked account that approved the login

## 8. Disable Or Unlink And Retry

Disable or unlink the same account in `login.divine.video`, then retry both refresh and a fresh browser authorization flow.

Expect:

- the existing refresh token is rejected immediately after disable
- a new authorization request is rejected because the account is no longer `ready`
- no new authorization code is issued

Operational note:

- already-issued access tokens are short-lived and may continue to work until expiry
- the current Phase 2 contract is immediate rejection for new approvals, not hard introspection of existing access tokens

## Evidence To Capture

- JSON output from both discovery endpoints
- PAR response payload
- final callback URL showing `code`, `state`, and `iss`
- token response showing `sub`
- refresh response showing token rotation
- `com.atproto.server.getSession` response showing the same DID
- rejection evidence after disable or unlink
