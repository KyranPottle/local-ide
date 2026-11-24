import { describe, expect, test } from "vitest"
import { parseMarkdown, safeLinkHref } from "./markdown"

describe("parseMarkdown", () => {
  test("headings, lists, emphasis, and inline code", () => {
    expect(parseMarkdown("## Plan\n\n- **one**\n- *two* `x<y`")).toEqual([
      { kind: "heading", level: 2, children: [{ kind: "text", text: "Plan" }] },
      {
        kind: "list",
        ordered: false,
        start: 1,
        items: [
          [{ kind: "paragraph", children: [{ kind: "strong", children: [{ kind: "text", text: "one" }] }] }],
          [{
            kind: "paragraph",
            children: [
              { kind: "emphasis", children: [{ kind: "text", text: "two" }] },
              { kind: "text", text: " " },
              { kind: "code", text: "x<y" },
            ],
          }],
        ],
      },
    ])
  })

  test("a fenced code block keeps its language and text", () => {
    expect(parseMarkdown("```ts\nconst a = 1 < 2\n```")).toEqual([{ kind: "code", language: "ts", text: "const a = 1 < 2" }])
  })

  test("a simple table", () => {
    expect(parseMarkdown("| a | b |\n|:-|-:|\n| 1 | 2 |")).toEqual([{
      kind: "table",
      alignments: ["left", "right"],
      header: [[{ kind: "text", text: "a" }], [{ kind: "text", text: "b" }]],
      rows: [[[{ kind: "text", text: "1" }], [{ kind: "text", text: "2" }]]],
    }])
  })

  test("an https link becomes a link", () => {
    expect(parseMarkdown("[docs](https://example.com/a)")).toEqual([{
      kind: "paragraph",
      children: [{ kind: "link", href: "https://example.com/a", children: [{ kind: "text", text: "docs" }] }],
    }])
  })

  test("inline HTML stays plain text", () => {
    expect(parseMarkdown("see <img src=x onerror=alert(1)>")).toEqual([{
      kind: "paragraph",
      children: [{ kind: "text", text: "see " }, { kind: "text", text: "<img src=x onerror=alert(1)>" }],
    }])
  })

  test("a script block stays plain text", () => {
    expect(parseMarkdown("<script>alert(1)</script>")).toEqual([
      { kind: "paragraph", children: [{ kind: "text", text: "<script>alert(1)</script>" }] },
    ])
  })

  test("a javascript link is not a link", () => {
    expect(parseMarkdown("[x](javascript:alert(1))")).toEqual([
      { kind: "paragraph", children: [{ kind: "text", text: "[x](javascript:alert(1))" }] },
    ])
  })

  test("a data link is not a link", () => {
    expect(parseMarkdown("[x](data:text/html,<b>a</b>)")).toEqual([
      { kind: "paragraph", children: [{ kind: "text", text: "[x](data:text/html,<b>a</b>)" }] },
    ])
  })

  test("an image shows as its source text", () => {
    expect(parseMarkdown("![a](https://example.com/a.png)")).toEqual([
      { kind: "paragraph", children: [{ kind: "text", text: "![a](https://example.com/a.png)" }] },
    ])
  })
})

describe("safeLinkHref", () => {
  test.each([
    ["https://example.com", "https://example.com/"],
    ["http://example.com/a", "http://example.com/a"],
    ["mailto:a@example.com", "mailto:a@example.com"],
    ["javascript:alert(1)", undefined],
    ["JavaScript:alert(1)", undefined],
    ["data:text/html,x", undefined],
    ["vbscript:x", undefined],
    ["/relative", undefined],
    ["file:///etc/passwd", undefined],
  ])("%s", (href, expected) => {
    expect(safeLinkHref(href)).toBe(expected)
  })
})
