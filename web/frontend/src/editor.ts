/// TipTap editor configuration with kerai-aware schema.
import { Editor } from '@tiptap/core'
import StarterKit from '@tiptap/starter-kit'
import Link from '@tiptap/extension-link'
import Image from '@tiptap/extension-image'
import CodeBlock from '@tiptap/extension-code-block'
import Table from '@tiptap/extension-table'
import TableRow from '@tiptap/extension-table-row'
import TableHeader from '@tiptap/extension-table-header'
import TableCell from '@tiptap/extension-table-cell'
import TaskList from '@tiptap/extension-task-list'
import TaskItem from '@tiptap/extension-task-item'

export function createEditor(element: HTMLElement, onChange?: () => void): Editor {
  const editor = new Editor({
    element,
    extensions: [
      StarterKit.configure({
        heading: {
          levels: [1, 2, 3, 4, 5, 6],
        },
        codeBlock: false, // use standalone extension
      }),
      Link.configure({
        openOnClick: false,
        HTMLAttributes: { rel: 'noopener noreferrer' },
      }),
      Image,
      CodeBlock.configure({
        HTMLAttributes: { class: 'code-block' },
      }),
      Table.configure({ resizable: true }),
      TableRow,
      TableHeader,
      TableCell,
      TaskList,
      TaskItem.configure({ nested: true }),
    ],
    content: '<p>Start writing...</p>',
    onUpdate: () => {
      onChange?.();
    },
  })

  return editor
}

/// Map TipTap node types to kerai kinds.
export const TIPTAP_TO_KERAI: Record<string, string> = {
  heading: 'heading',
  paragraph: 'paragraph',
  blockquote: 'blockquote',
  bulletList: 'list',
  orderedList: 'list',
  listItem: 'list_item',
  codeBlock: 'code_block',
  table: 'table',
  tableRow: 'table_row',
  tableHeader: 'table_head',
  tableCell: 'table_cell',
  taskList: 'list',
  taskItem: 'list_item',
  image: 'image',
  horizontalRule: 'thematic_break',
}

/// Map kerai kinds back to TipTap node types.
export const KERAI_TO_TIPTAP: Record<string, string> = {
  heading: 'heading',
  paragraph: 'paragraph',
  blockquote: 'blockquote',
  list: 'bulletList',
  list_item: 'listItem',
  code_block: 'codeBlock',
  table: 'table',
  table_row: 'tableRow',
  table_head: 'tableHeader',
  table_cell: 'tableCell',
  image: 'image',
  thematic_break: 'horizontalRule',
}
