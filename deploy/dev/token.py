#!/usr/bin/env python3
"""Prints a real access token for one of the seeded people (development only): deploy/dev/token.py ana"""
import json, os, sys, urllib.error, urllib.parse, urllib.request

who = sys.argv[1]
acc = json.load(open(os.path.join(os.path.dirname(__file__), "..", "..", ".dev", "accounts.json")))
form = urllib.parse.urlencode({"grant_type": "password", "client_id": "loamz-data-test", "username": who, "password": acc[who]["password"], "scope": "openid organization:*"}).encode()
base = os.environ.get("KEYCLOAK_URL", "http://127.0.0.1:8082")
try:
    print(json.load(urllib.request.urlopen(urllib.request.Request(f"{base}/realms/loamz/protocol/openid-connect/token", data=form)))["access_token"])
except urllib.error.HTTPError as e:
    sys.exit(f"Keycloak refused: {e.read().decode()[:300]}")
