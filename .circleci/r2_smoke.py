#!/usr/bin/env python3
"""Verify R2 S3-compatible credentials work without installing aws-cli.

Usage:
    R2_ACCOUNT_ID=...        \
    R2_ACCESS_KEY_ID=...     \
    R2_SECRET_ACCESS_KEY=... \
    python3 .circleci/r2_smoke.py

Or pass them as args:
    python3 .circleci/r2_smoke.py <account_id> <access_key> <secret_key>

The script signs a `GET /artui-releases/` request with SigV4 and reports
the response. If the credentials are correct, you'll see HTTP 200 +
either a list of objects or an empty bucket. If they're wrong, you'll
see exactly which step failed (SignatureDoesNotMatch, 403, etc) without
the noise of an aws-cli upload.

Pure stdlib (hashlib, hmac, urllib) so this runs anywhere with Python
3.8+. Useful as a local diagnostic when CircleCI's upload fails — if
the keys work locally but not on CI, the problem is on CircleCI's side
(env var trimming, hidden whitespace, etc).
"""

from __future__ import annotations

import datetime as dt
import hashlib
import hmac
import os
import sys
import urllib.error
import urllib.request


def sign(key: bytes, msg: str) -> bytes:
    return hmac.new(key, msg.encode("utf-8"), hashlib.sha256).digest()


def derive_signing_key(secret: str, date_stamp: str, region: str, service: str) -> bytes:
    k_date = sign(("AWS4" + secret).encode("utf-8"), date_stamp)
    k_region = sign(k_date, region)
    k_service = sign(k_region, service)
    return sign(k_service, "aws4_request")


def main(argv: list[str]) -> int:
    if len(argv) >= 4:
        account_id, access_key, secret_key = argv[1], argv[2], argv[3]
    else:
        account_id = os.environ.get("R2_ACCOUNT_ID", "").strip()
        access_key = os.environ.get("R2_ACCESS_KEY_ID", "").strip()
        secret_key = os.environ.get("R2_SECRET_ACCESS_KEY", "").strip()

    if not account_id or not access_key or not secret_key:
        print(
            "ERROR: missing one or more of R2_ACCOUNT_ID / R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY",
            file=sys.stderr,
        )
        print(
            "       Set as env vars or pass as positional args.",
            file=sys.stderr,
        )
        return 1

    if len(access_key) != 32:
        print(
            f"WARNING: access key is {len(access_key)} chars (expected 32). "
            "Likely a Cloudflare API token (cfat-...) instead of an R2 S3 key.",
            file=sys.stderr,
        )
    if len(secret_key) != 64:
        print(
            f"WARNING: secret key is {len(secret_key)} chars (expected 64).",
            file=sys.stderr,
        )

    host = f"{account_id}.r2.cloudflarestorage.com"
    bucket = "artui-releases"
    region = "auto"
    service = "s3"
    method = "GET"

    # Build the canonical request for `GET /<bucket>/`. Empty payload.
    now = dt.datetime.now(dt.timezone.utc)
    amz_date = now.strftime("%Y%m%dT%H%M%SZ")
    date_stamp = now.strftime("%Y%m%d")

    canonical_uri = f"/{bucket}/"
    canonical_querystring = "list-type=2"
    payload_hash = hashlib.sha256(b"").hexdigest()
    canonical_headers = (
        f"host:{host}\n"
        f"x-amz-content-sha256:{payload_hash}\n"
        f"x-amz-date:{amz_date}\n"
    )
    signed_headers = "host;x-amz-content-sha256;x-amz-date"
    canonical_request = "\n".join(
        [
            method,
            canonical_uri,
            canonical_querystring,
            canonical_headers,
            signed_headers,
            payload_hash,
        ]
    )

    credential_scope = f"{date_stamp}/{region}/{service}/aws4_request"
    string_to_sign = "\n".join(
        [
            "AWS4-HMAC-SHA256",
            amz_date,
            credential_scope,
            hashlib.sha256(canonical_request.encode("utf-8")).hexdigest(),
        ]
    )

    signing_key = derive_signing_key(secret_key, date_stamp, region, service)
    signature = hmac.new(
        signing_key, string_to_sign.encode("utf-8"), hashlib.sha256
    ).hexdigest()

    authorization = (
        f"AWS4-HMAC-SHA256 Credential={access_key}/{credential_scope}, "
        f"SignedHeaders={signed_headers}, Signature={signature}"
    )

    url = f"https://{host}{canonical_uri}?{canonical_querystring}"
    print(f"GET {url}")
    print(f"  account_id [{account_id[:4]}…{account_id[-4:]}] ({len(account_id)} chars)")
    print(f"  access_key [{access_key[:4]}…{access_key[-4:]}] ({len(access_key)} chars)")
    print(f"  secret_key [{secret_key[:4]}…{secret_key[-4:]}] ({len(secret_key)} chars)")
    print()

    request = urllib.request.Request(
        url,
        method=method,
        headers={
            "Host": host,
            "x-amz-content-sha256": payload_hash,
            "x-amz-date": amz_date,
            "Authorization": authorization,
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=15) as resp:
            print(f"HTTP {resp.status} — credentials are GOOD.")
            body = resp.read(2048)
            print(body.decode("utf-8", errors="replace"))
            return 0
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace")
        print(f"HTTP {e.code} — request rejected.")
        print(body)
        # Hint extraction
        if "SignatureDoesNotMatch" in body:
            print()
            print("Diagnosis: secret key is wrong (or has stray whitespace, or was")
            print("paired with an access key from a different roll). Roll the token")
            print("in Cloudflare R2 dashboard, copy *both* values from the new")
            print("display, and update both env vars.")
        elif "InvalidAccessKeyId" in body:
            print()
            print("Diagnosis: access key doesn't exist or was deleted. Check that")
            print("you copied the 32-char Access Key ID, not the cfat-... Token value.")
        elif "AccessDenied" in body:
            print()
            print("Diagnosis: keys are valid but lack permission on this bucket.")
            print("Re-create the token with Object Read & Write scoped to artui-releases.")
        return 2
    except urllib.error.URLError as e:
        print(f"Network error: {e}")
        return 3


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
