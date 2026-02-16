/// Fetch wrapper for /api/* endpoints.

const BASE = '/api';

async function request<T>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...opts,
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`${res.status}: ${text}`);
  }
  return res.json();
}

export interface Document {
  id: string;
  content: string;
  metadata: Record<string, unknown>;
  created_at: string;
}

export interface TreeNode {
  id: string;
  kind: string;
  content: string | null;
  parent_id: string | null;
  position: number;
  metadata: Record<string, unknown>;
  depth: number;
}

export interface SearchResult {
  id: string;
  kind: string;
  content: string;
  path: string;
  rank?: number;
  combined_score?: number;
  agents?: Array<{ agent: string; weight: number; reasoning: string }>;
}

// Documents
export const listDocuments = () => request<Document[]>('/documents');

export const createDocument = (source: string, filename: string) =>
  request<{ file: string; nodes: number; edges: number }>('/documents', {
    method: 'POST',
    body: JSON.stringify({ source, filename }),
  });

export const getDocumentTree = (id: string) =>
  request<TreeNode[]>(`/documents/${id}/tree`);

export const getDocumentMarkdown = async (id: string): Promise<string> => {
  const res = await fetch(`${BASE}/documents/${id}/markdown`);
  if (!res.ok) throw new Error(`${res.status}: ${await res.text()}`);
  return res.text();
};

// Nodes
export const applyOp = (op_type: string, node_id: string | null, payload: Record<string, unknown>) =>
  request<{ op_type: string; node_id: string }>('/nodes', {
    method: 'POST',
    body: JSON.stringify({ op_type, node_id, payload }),
  });

export const updateContent = (nodeId: string, content: string) =>
  request<{ op_type: string; node_id: string }>(`/nodes/${nodeId}/content`, {
    method: 'PATCH',
    body: JSON.stringify({ content }),
  });

export const deleteNode = (nodeId: string) =>
  request<{ op_type: string; node_id: string }>(`/nodes/${nodeId}`, {
    method: 'DELETE',
  });

// Search
export const search = (q: string, kind?: string, limit?: number) => {
  const params = new URLSearchParams({ q });
  if (kind) params.set('kind', kind);
  if (limit) params.set('limit', String(limit));
  return request<SearchResult[]>(`/search?${params}`);
};

export const suggest = (text: string, agents?: string, limit?: number) => {
  const params = new URLSearchParams({ text });
  if (agents) params.set('agents', agents);
  if (limit) params.set('limit', String(limit));
  return request<SearchResult[]>(`/suggest?${params}`);
};
