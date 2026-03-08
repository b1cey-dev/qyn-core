import pytest
from quyn import QuynClient


def test_client_default_url():
    c = QuynClient()
    assert c.url.endswith("/rpc")


def test_client_custom_url():
    c = QuynClient("http://localhost:8545")
    assert "localhost" in c.url
