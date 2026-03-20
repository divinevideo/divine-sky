#!/bin/sh
set -eu

alias_name="${MINIO_ALIAS:-local}"
endpoint="${MINIO_ENDPOINT:-http://minio:9000}"
root_user="${MINIO_ROOT_USER:-minioadmin}"
root_password="${MINIO_ROOT_PASSWORD:-minioadmin}"
buckets="${MINIO_BUCKETS:-pds-blobs}"

mc alias set "$alias_name" "$endpoint" "$root_user" "$root_password"

IFS=','
for bucket in $buckets; do
  trimmed_bucket="$(printf '%s' "$bucket" | tr -d '[:space:]')"
  [ -n "$trimmed_bucket" ] || continue
  mc mb --ignore-existing "$alias_name/$trimmed_bucket"
done
