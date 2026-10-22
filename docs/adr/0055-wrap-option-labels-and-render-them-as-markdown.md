# 55. Wrap option labels and render them as Markdown

Date: 2026-10-22

## Status

Accepted

Amends [35. Render the text fields as full Markdown through patine](0035-render-the-text-fields-as-full-markdown-through-patine.md)

## Context

An option's label was one line of plain text after the option's
number. Nothing wrapped it, and the question pane, which never gets
wider than the reading width (ADR 30), cut it at its edge without a
mark of the cut. In a `multi` question whose options each carried a
whole review remark in `label`, every option row ended mid-word,
although the terminal was far wider, so the person could not read what
they were asked to pick. Backticks showed raw, since labels were not
Markdown. Neither the spec nor the agent skill said that a label should
be short.

## Decision

- **Labels are text fields:** an option's label renders full Markdown
  like the intro, question texts and option descriptions (ADR 35),
  through the same call to patine, in bold over the styles Markdown
  gives it.
- **Labels wrap:** the label follows the option's number and wraps
  under its own start, rendered for the columns from there to the end
  of the line.
- **A label is never cut across the width,** however little room there
  is. A word wider than the room continues behind a dim `↪`, as in every
  text field. In a pane so narrow that the number leaves the label less
  than 20 columns, the whole label goes on the lines below the option's
  row, from column 6, or from column 0 in a pane narrower than 8
  columns. A field's text in the same case keeps its first line after
  its head (ADR 36); a label moves whole.
- **Descriptions:** they stand beside the labels, in one column after
  the widest label, only while every label of the question takes a
  single line beside its number and the column leaves the descriptions
  30 columns. Otherwise every description of the question goes below
  its label, from column 6, after the label's last line. A label without
  any text ends where its number does.
- **The fill:** under the cursor, the focus fill covers the pointer,
  the mark, the number and every line of the label.
- **Height:** an option taller than the question's area shows from its
  start, as any row does (spec section 7.4). Its later lines may lie out of reach;
  long text belongs in the description or the context.
- **The agent skill** calls a label the short handle the person picks
  the option by, and sends longer text to the option's `description`, or
  to its `context` when it is material to read. The schema's description
  of `label` says the same.

## Consequences

- A label starting with `- `, `# `, `> ` or `1. ` renders as a list
  item, heading, quote or numbered item, and `*x*` shows as emphasis
  without its asterisks.
- Every frame renders each label through patine, as it renders the
  other text fields (ADR 35).
