/// AI suggestions sidebar — queries context_search as user types.
import { suggest, type SearchResult } from './api'

let debounceTimer: number | null = null;

export function initSidebar(): void {
  const searchInput = document.getElementById('search-input') as HTMLInputElement;
  const suggestionsDiv = document.getElementById('suggestions')!;

  searchInput.addEventListener('input', () => {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = window.setTimeout(async () => {
      const query = searchInput.value.trim();
      if (query.length < 2) {
        suggestionsDiv.innerHTML = '<p style="color:#999;font-size:13px">Type to search...</p>';
        return;
      }

      try {
        const results = await suggest(query, undefined, 10);
        renderSuggestions(suggestionsDiv, results);
      } catch (e) {
        suggestionsDiv.innerHTML = '<p style="color:#c00;font-size:13px">Search error</p>';
      }
    }, 500);
  });

  suggestionsDiv.innerHTML = '<p style="color:#999;font-size:13px">Type to search...</p>';
}

function renderSuggestions(container: HTMLElement, results: SearchResult[]): void {
  if (results.length === 0) {
    container.innerHTML = '<p style="color:#999;font-size:13px">No results</p>';
    return;
  }

  container.innerHTML = results.map(r => `
    <div class="suggestion-item" data-id="${r.id}">
      <div class="kind">${r.kind}</div>
      <div class="content">${escapeHtml(r.content || '')}</div>
      ${r.path ? `<div class="path">${escapeHtml(r.path)}</div>` : ''}
      ${r.combined_score ? `<div class="path">score: ${r.combined_score.toFixed(3)}</div>` : ''}
      ${r.agents && r.agents.length > 0 ? `<div class="path">${r.agents.map(a => `${a.agent}: ${a.weight}`).join(', ')}</div>` : ''}
    </div>
  `).join('');
}

function escapeHtml(text: string): string {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}
