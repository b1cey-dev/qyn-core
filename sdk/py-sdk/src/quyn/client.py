"""Quyn RPC client - Ethereum-compatible JSON-RPC."""

import requests
from typing import Any, List, Optional

DEFAULT_RPC = "http://127.0.0.1:8545"


class QuynClient:
    """Client for Quyn node JSON-RPC (chainId 7777)."""

    def __init__(self, rpc_url: str = DEFAULT_RPC):
        self.url = rpc_url.rstrip("/") + "/rpc"
        self._id = 0

    def request(self, method: str, params: Optional[List[Any]] = None) -> Any:
        payload = {
            "jsonrpc": "2.0",
            "id": self._id,
            "method": method,
            "params": params or [],
        }
        self._id += 1
        r = requests.post(self.url, json=payload, timeout=10)
        r.raise_for_status()
        data = r.json()
        if "error" in data:
            raise RuntimeError(data["error"].get("message", "RPC error"))
        return data.get("result")

    def get_block_number(self) -> str:
        return self.request("eth_blockNumber")

    def get_chain_id(self) -> str:
        return self.request("eth_chainId")

    def net_version(self) -> str:
        return self.request("net_version")
