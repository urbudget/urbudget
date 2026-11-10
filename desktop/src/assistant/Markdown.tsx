import { Fragment, memo, type ReactNode } from "react";

/**
 * The Markdown the assistant writes: paragraphs, headings, bullet and numbered lists (one level
 * of nesting), tables, quotes, code, **bold**, *italic* and `code`. Never HTML: everything is
 * rendered as text. While a reply is streaming, an unfinished **bold** or `code` run is shown
 * as if it were closed, so markers never flash on screen.
 */

type Block =
  | { kind: "p"; lines: string[] }
  | { kind: "h"; text: string }
  | { kind: "list"; ordered: boolean; start: number; items: { text: string; children: string[] }[] }
  | { kind: "table"; head: string[]; rows: string[][]; align: ("left" | "right" | "center")[] }
  | { kind: "quote"; lines: string[] }
  | { kind: "code"; text: string }
  | { kind: "hr" };

const BULLET = /^(\s*)([-*•+])\s+(.*)$/;
const NUMBER = /^(\s*)(\d+)[.)]\s+(.*)$/;
const cells = (line: string) =>
  line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((c) => c.trim());
const isDivider = (line: string) => /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(line);

export function blocks(text: string): Block[] {
  const lines = text.replace(/\r/g, "").split("\n");
  const out: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i++;
      continue;
    }
    if (/^\s*```/.test(line)) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !/^\s*```/.test(lines[i])) body.push(lines[i++]);
      i++;
      out.push({ kind: "code", text: body.join("\n") });
      continue;
    }
    if (/^\s*#{1,6}\s+/.test(line)) {
      out.push({ kind: "h", text: line.replace(/^\s*#{1,6}\s+/, "").replace(/\s*#+\s*$/, "") });
      i++;
      continue;
    }
    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) {
      out.push({ kind: "hr" });
      i++;
      continue;
    }
    if (line.trim().startsWith("|") && i + 1 < lines.length && isDivider(lines[i + 1])) {
      const head = cells(line);
      const align = cells(lines[i + 1]).map((c) =>
        c.startsWith(":") && c.endsWith(":") ? "center" : c.endsWith(":") ? "right" : "left",
      ) as ("left" | "right" | "center")[];
      const rows: string[][] = [];
      i += 2;
      while (i < lines.length && lines[i].trim().startsWith("|")) rows.push(cells(lines[i++]));
      out.push({ kind: "table", head, rows, align });
      continue;
    }
    if (/^\s*>/.test(line)) {
      const quoted: string[] = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) quoted.push(lines[i++].replace(/^\s*>\s?/, ""));
      out.push({ kind: "quote", lines: quoted });
      continue;
    }
    const bullet = BULLET.exec(line);
    const number = NUMBER.exec(line);
    if (bullet || number) {
      const ordered = !bullet;
      const indent = (bullet ?? number)![1].length;
      const items: { text: string; children: string[] }[] = [];
      while (i < lines.length) {
        const l = lines[i];
        const b = BULLET.exec(l);
        const n = NUMBER.exec(l);
        const m = b ?? n;
        if (m && m[1].length <= indent + 1 && (ordered ? !!n : !!b)) {
          items.push({ text: m[3], children: [] });
          i++;
        } else if (m && m[1].length > indent + 1 && items.length) {
          items[items.length - 1].children.push(m[3]);
          i++;
        } else if (l.trim() && /^\s{2,}/.test(l) && items.length) {
          // A continuation line of the item above.
          items[items.length - 1].text += " " + l.trim();
          i++;
        } else if (!l.trim() && i + 1 < lines.length && (BULLET.test(lines[i + 1]) || NUMBER.test(lines[i + 1]))) {
          // A blank line between items keeps the list going.
          i++;
        } else break;
      }
      out.push({ kind: "list", ordered, start: number ? Number(number[2]) : 1, items });
      continue;
    }
    const paragraph: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() &&
      !BULLET.test(lines[i]) &&
      !NUMBER.test(lines[i]) &&
      !/^\s*(#{1,6}\s|```|>)/.test(lines[i]) &&
      !(lines[i].trim().startsWith("|") && i + 1 < lines.length && isDivider(lines[i + 1]))
    )
      paragraph.push(lines[i++]);
    out.push({ kind: "p", lines: paragraph });
  }
  return out;
}

/** Closes an unfinished ** or ` run at the end of streamed text, and hides a marker that hasn't opened a word yet. */
function closeOpen(text: string) {
  let t = text.replace(/(^|\s)(\*{1,2}|`)$/, "$1").replace(/\n\s*([-*•+]|\d+[.)])\s*$/, "\n");
  if ((t.match(/`/g)?.length ?? 0) % 2) t += "`";
  const outside = t.replace(/`[^`]*`/g, "");
  if ((outside.match(/\*\*/g)?.length ?? 0) % 2) t += "**";
  return t;
}

export function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  const pattern = /(`[^`]+`)|(\*\*[^*]+?\*\*|__[^_]+?__)|(\*[^*\s][^*]*?\*|(?<![\w])_[^_\s][^_]*?_(?![\w]))|(\[([^\]]+)\]\(([^)\s]+)\))/g;
  let last = 0;
  let key = 0;
  for (const m of text.matchAll(pattern)) {
    if (m.index! > last) out.push(<Fragment key={key++}>{text.slice(last, m.index)}</Fragment>);
    if (m[1]) out.push(<code key={key++}>{m[1].slice(1, -1)}</code>);
    else if (m[2]) out.push(<strong key={key++}>{inline(m[2].slice(2, -2))}</strong>);
    else if (m[3]) out.push(<em key={key++}>{inline(m[3].slice(1, -1))}</em>);
    // Links are shown as their text; the address is in the tooltip, never opened from here.
    else if (m[4])
      out.push(
        <span key={key++} className="md-link" title={m[6]}>
          {m[5]}
        </span>,
      );
    last = m.index! + m[0].length;
  }
  if (last < text.length) out.push(<Fragment key={key++}>{text.slice(last)}</Fragment>);
  return out;
}

/** A column's side as the table reads: "left" is where a row starts, "right" (amounts) where it ends. */
const side = (align: "left" | "right" | "center") => (align === "left" ? "start" : align === "right" ? "end" : "center");

// Each block takes its direction from its own text (dir="auto"), so an Arabic paragraph, list or
// table reads right to left in the English layout and an English one left to right in the Arabic.
// A list takes one direction for all its items, so its markers stay in its own margin.
function MarkdownView({ text, streaming = false }: { text: string; streaming?: boolean }) {
  const source = streaming ? closeOpen(text) : text;
  return (
    <>
      {blocks(source).map((b, i) => {
        switch (b.kind) {
          case "p":
            return (
              <p key={i} dir="auto">
                {b.lines.map((l, j) => (
                  <Fragment key={j}>
                    {j > 0 && <br />}
                    {inline(l)}
                  </Fragment>
                ))}
              </p>
            );
          case "h":
            return (
              <h4 key={i} className="md-heading" dir="auto">
                {inline(b.text)}
              </h4>
            );
          case "list": {
            const items = b.items.map((item, j) => (
              <li key={j}>
                {inline(item.text)}
                {item.children.length > 0 && (
                  <ul>
                    {item.children.map((c, k) => (
                      <li key={k}>{inline(c)}</li>
                    ))}
                  </ul>
                )}
              </li>
            ));
            return b.ordered ? (
              <ol key={i} start={b.start} dir="auto">
                {items}
              </ol>
            ) : (
              <ul key={i} dir="auto">
                {items}
              </ul>
            );
          }
          case "table":
            return (
              <div key={i} className="md-table">
                <table dir="auto">
                  <thead>
                    <tr>
                      {b.head.map((h, j) => (
                        <th key={j} style={{ textAlign: side(b.align[j] ?? "left") }}>
                          {inline(h)}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {b.rows.map((r, j) => (
                      <tr key={j}>
                        {b.head.map((_, k) => (
                          <td key={k} style={{ textAlign: side(b.align[k] ?? "left") }}>
                            {inline(r[k] ?? "")}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
          case "quote":
            return (
              <blockquote key={i} dir="auto">
                {b.lines.map((l, j) => (
                  <Fragment key={j}>
                    {j > 0 && <br />}
                    {inline(l)}
                  </Fragment>
                ))}
              </blockquote>
            );
          case "code":
            return (
              // Code reads left to right whatever is around it.
              <pre key={i} className="md-code" dir="ltr">
                {b.text}
              </pre>
            );
          case "hr":
            return <hr key={i} />;
        }
      })}
    </>
  );
}

/** Re-renders only when its text changes. */
export const Markdown = memo(MarkdownView);
