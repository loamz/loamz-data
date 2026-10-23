"""The session policy a statement's storage credentials carry (no object store needed)."""

from __future__ import annotations

import json
from datetime import UTC, datetime, timedelta

from loamz.server.data.features.tables.credentials import CredentialVendor
from loamz.server.data.kernel.settings import Settings

KEY = "arn:aws:kms:eu-west-1:111122223333:key/abcd"


class FakeSts:
    def __init__(self) -> None:
        self.calls: list[dict] = []

    def assume_role(self, **kwargs):
        self.calls.append(kwargs)
        return {
            "Credentials": {
                "AccessKeyId": "AK",
                "SecretAccessKey": "SK",
                "SessionToken": "ST",
                "Expiration": datetime.now(UTC) + timedelta(hours=1),
            }
        }


def policy_for(**extra) -> dict:
    sts = FakeSts()
    settings = Settings(
        _env_file=None, warehouse="warehouse", storage_sts_role_arn="arn:aws:iam::1:role/v", **extra
    )
    CredentialVendor(settings, client=sts).for_prefixes(["s3://warehouse/ns/t1/"])
    return json.loads(sts.calls[0]["Policy"])


def test_credentials_read_only_the_planned_folders():
    statements = policy_for()["Statement"]
    assert len(statements) == 1
    assert statements[0]["Action"] == ["s3:GetObject"]
    assert statements[0]["Resource"] == ["arn:aws:s3:::warehouse/ns/t1/*"]


def test_a_customer_managed_key_adds_decrypt_on_that_key_only():
    statements = policy_for(storage_kms_key_arn=KEY)["Statement"]
    assert statements[1] == {"Effect": "Allow", "Action": ["kms:Decrypt"], "Resource": [KEY]}
