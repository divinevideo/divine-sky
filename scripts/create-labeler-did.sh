#!/usr/bin/env bash
# Create a did:plc for the divine-labeler service.
#
# Usage:
#   LABELER_SIGNING_KEY=<hex> PDS_ENDPOINT=<url> HANDLE=<handle> ./scripts/create-labeler-did.sh
#
# Example:
#   LABELER_SIGNING_KEY=67b6ae7ec6c0fe33f443ec45b67c04a3fda77b697e9c4bb11593081375f64800 \
#   PDS_ENDPOINT=https://pds.staging.dvines.org \
#   HANDLE=labeler.staging.dvines.org \
#   PLC_DIRECTORY=https://plc.directory \
#   ./scripts/create-labeler-did.sh

set -euo pipefail

: "${LABELER_SIGNING_KEY:?LABELER_SIGNING_KEY must be set (64 hex chars)}"
: "${PDS_ENDPOINT:?PDS_ENDPOINT must be set}"
: "${HANDLE:?HANDLE must be set}"
: "${PLC_DIRECTORY:=https://plc.directory}"

echo "Creating labeler DID..."
echo "  Handle: ${HANDLE}"
echo "  PDS: ${PDS_ENDPOINT}"
echo "  PLC Directory: ${PLC_DIRECTORY}"

# This script delegates to a Rust binary that handles:
# 1. Deriving the public key from LABELER_SIGNING_KEY
# 2. Building a PLC operation with atproto_labeler service type
# 3. Signing the operation
# 4. POSTing to PLC directory
# 5. Printing the resulting DID

cargo run -p divine-labeler --bin create-labeler-did -- \
  --signing-key "${LABELER_SIGNING_KEY}" \
  --pds-endpoint "${PDS_ENDPOINT}" \
  --handle "${HANDLE}" \
  --plc-directory "${PLC_DIRECTORY}"
