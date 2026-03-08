/**
 * Quyn RPC client - Ethereum-compatible JSON-RPC (chainId 7777).
 */

const DEFAULT_RPC = 'http://127.0.0.1:8545';

export interface RpcRequest {
  jsonrpc: '2.0';
  id: number | string;
  method: string;
  params?: unknown[];
}

export interface RpcResponse<T = unknown> {
  jsonrpc: '2.0';
  id: number | string;
  result?: T;
  error?: { code: number; message: string };
}

export class QuynClient {
  private url: string;
  private id: number = 0;

  constructor(rpcUrl: string = DEFAULT_RPC) {
    this.url = rpcUrl;
  }

  async request<T = unknown>(method: string, params: unknown[] = []): Promise<T> {
    const body: RpcRequest = {
      jsonrpc: '2.0',
      id: ++this.id,
      method,
      params: params.length ? params : undefined,
    };
    const res = await fetch(this.url + '/rpc', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    const data: RpcResponse<T> = await res.json();
    if (data.error) {
      throw new Error(data.error.message || 'RPC error');
    }
    return data.result as T;
  }

  async getBlockNumber(): Promise<string> {
    return this.request<string>('eth_blockNumber');
  }

  async getChainId(): Promise<string> {
    return this.request<string>('eth_chainId');
  }

  async netVersion(): Promise<string> {
    return this.request<string>('net_version');
  }
}
