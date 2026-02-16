/// Main entry point — mount TipTap editor and wire up UI.
import { createEditor } from './editor'
import { WsClient } from './ws'
import { initSidebar } from './sidebar'
import * as api from './api'

// State
let currentDocId: string | null = null;
let saveTimer: number | null = null;

// Initialize editor
const editorEl = document.getElementById('editor')!;
const editor = createEditor(editorEl, onContentChange);

// Initialize WebSocket
const ws = new WsClient();
ws.connect();

// Handle remote updates
ws.onMessage((payload) => {
  console.log('[ws] remote op:', payload);
  // For now, just log. In a full implementation, we'd apply
  // the remote change to the editor state if it affects our document.
});

// Initialize sidebar
initSidebar();

// Load document list
loadDocuments();

// Toolbar buttons
document.getElementById('btn-h1')?.addEventListener('click', () =>
  editor.chain().focus().toggleHeading({ level: 1 }).run());
document.getElementById('btn-h2')?.addEventListener('click', () =>
  editor.chain().focus().toggleHeading({ level: 2 }).run());
document.getElementById('btn-h3')?.addEventListener('click', () =>
  editor.chain().focus().toggleHeading({ level: 3 }).run());
document.getElementById('btn-bold')?.addEventListener('click', () =>
  editor.chain().focus().toggleBold().run());
document.getElementById('btn-italic')?.addEventListener('click', () =>
  editor.chain().focus().toggleItalic().run());
document.getElementById('btn-code')?.addEventListener('click', () =>
  editor.chain().focus().toggleCodeBlock().run());
document.getElementById('btn-bullet')?.addEventListener('click', () =>
  editor.chain().focus().toggleBulletList().run());
document.getElementById('btn-blockquote')?.addEventListener('click', () =>
  editor.chain().focus().toggleBlockquote().run());

// Save button
document.getElementById('btn-save')?.addEventListener('click', saveDocument);

// Document selector
document.getElementById('doc-select')?.addEventListener('change', (e) => {
  const select = e.target as HTMLSelectElement;
  if (select.value) {
    loadDocument(select.value);
  } else {
    editor.commands.setContent('<p>Start writing...</p>');
    currentDocId = null;
  }
});

// Content change handler (debounced auto-save)
function onContentChange(): void {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    if (currentDocId) {
      saveDocument();
    }
  }, 2000);
}

async function loadDocuments(): Promise<void> {
  try {
    const docs = await api.listDocuments();
    const select = document.getElementById('doc-select') as HTMLSelectElement;
    // Clear existing options except the first
    while (select.options.length > 1) select.remove(1);

    for (const doc of docs) {
      const opt = document.createElement('option');
      opt.value = doc.id;
      opt.textContent = doc.content || 'Untitled';
      select.appendChild(opt);
    }
  } catch (e) {
    console.error('Failed to load documents:', e);
  }
}

async function loadDocument(docId: string): Promise<void> {
  try {
    const markdown = await api.getDocumentMarkdown(docId);
    // Convert markdown to HTML for TipTap
    // Simple conversion for now — TipTap can parse HTML
    const html = markdownToHtml(markdown);
    editor.commands.setContent(html);
    currentDocId = docId;
  } catch (e) {
    console.error('Failed to load document:', e);
  }
}

async function saveDocument(): Promise<void> {
  const html = editor.getHTML();
  // Convert editor HTML back to markdown for storage
  // For now, save as-is via parse_markdown with a simple extraction
  const text = editor.getText();
  const filename = currentDocId
    ? (document.getElementById('doc-select') as HTMLSelectElement).selectedOptions[0]?.textContent || 'untitled.md'
    : `doc-${Date.now()}.md`;

  try {
    const result = await api.createDocument(text, filename);
    console.log('Saved:', result);

    // Refresh document list
    await loadDocuments();

    // If new document, select it
    if (!currentDocId) {
      await loadDocuments();
    }
  } catch (e) {
    console.error('Failed to save:', e);
  }
}

// Simple markdown to HTML converter for loading
function markdownToHtml(md: string): string {
  return md
    .replace(/^### (.+)$/gm, '<h3>$1</h3>')
    .replace(/^## (.+)$/gm, '<h2>$1</h2>')
    .replace(/^# (.+)$/gm, '<h1>$1</h1>')
    .replace(/^> (.+)$/gm, '<blockquote><p>$1</p></blockquote>')
    .replace(/^- (.+)$/gm, '<li>$1</li>')
    .replace(/(<li>.*<\/li>\n?)+/g, '<ul>$&</ul>')
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/\*(.+?)\*/g, '<em>$1</em>')
    .replace(/`(.+?)`/g, '<code>$1</code>')
    .replace(/\[(.+?)\]\((.+?)\)/g, '<a href="$2">$1</a>')
    .replace(/^(?!<[hubloa])(.*\S.*)$/gm, '<p>$1</p>')
    .replace(/\n{2,}/g, '\n');
}
