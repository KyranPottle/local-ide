import { createMemo, For, Index, type JSX } from "solid-js"
import { Dynamic } from "solid-js/web"
import { CopyButton } from "./CopyButton"
import { type MarkdownBlock, type MarkdownInline, parseMarkdown } from "./markdown"

function renderInline(node: MarkdownInline): JSX.Element {
  switch (node.kind) {
    case "text":
      return <>{node.text}</>
    case "strong":
      return <strong><InlineList inlines={node.children} /></strong>
    case "emphasis":
      return <em><InlineList inlines={node.children} /></em>
    case "strikethrough":
      return <del><InlineList inlines={node.children} /></del>
    case "code":
      return <code class="markdown-inline-code">{node.text}</code>
    case "link":
      return (
        <a href={node.href} target="_blank" rel="noopener noreferrer">
          <InlineList inlines={node.children} />
        </a>
      )
    case "line_break":
      return <br />
    default: {
      const unhandled: never = node
      return unhandled
    }
  }
}

function InlineList(props: { inlines: readonly MarkdownInline[] }) {
  return <Index each={props.inlines}>{(inline) => <>{renderInline(inline())}</>}</Index>
}

function CodeBlock(props: { language: string, text: string }) {
  return (
    <figure class="markdown-code">
      <figcaption class="markdown-code-header">
        <span>{props.language || "text"}</span>
        <CopyButton text={props.text} label="Copy code" />
      </figcaption>
      <pre><code>{props.text}</code></pre>
    </figure>
  )
}

function renderBlock(node: MarkdownBlock): JSX.Element {
  switch (node.kind) {
    case "heading":
      return (
        <Dynamic component={`h${Math.min(Math.max(node.level, 1), 6)}`} class="markdown-heading">
          <InlineList inlines={node.children} />
        </Dynamic>
      )
    case "paragraph":
      return <p><InlineList inlines={node.children} /></p>
    case "code":
      return <CodeBlock language={node.language} text={node.text} />
    case "list":
      return (
        <Dynamic component={node.ordered ? "ol" : "ul"} class="markdown-list" start={node.ordered ? node.start : undefined}>
          <For each={node.items}>{(item) => <li><BlockList blocks={item} /></li>}</For>
        </Dynamic>
      )
    case "quote":
      return <blockquote><BlockList blocks={node.children} /></blockquote>
    case "table":
      return (
        <div class="markdown-table-scroll">
          <table>
            <thead>
              <tr>
                <For each={node.header}>
                  {(cell, column) => <th style={{ "text-align": node.alignments[column()] ?? undefined }}><InlineList inlines={cell} /></th>}
                </For>
              </tr>
            </thead>
            <tbody>
              <For each={node.rows}>
                {(row) => (
                  <tr>
                    <For each={row}>
                      {(cell, column) => <td style={{ "text-align": node.alignments[column()] ?? undefined }}><InlineList inlines={cell} /></td>}
                    </For>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </div>
      )
    case "rule":
      return <hr />
    default: {
      const unhandled: never = node
      return unhandled
    }
  }
}

function BlockList(props: { blocks: readonly MarkdownBlock[] }) {
  return <Index each={props.blocks}>{(block) => <>{renderBlock(block())}</>}</Index>
}

export function MarkdownView(props: { text: string }) {
  const blocks = createMemo(() => parseMarkdown(props.text))
  return <div class="markdown"><BlockList blocks={blocks()} /></div>
}
