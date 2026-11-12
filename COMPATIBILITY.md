# Rustium compatibility

Status as of Phase 2. "Tested" means an automated test in this repository
exercises the behavior and passes. Nothing here is a claim about speed,
security, or compatibility with other browsers.

## HTML parsing (`rustium-html`, built on html5ever)

### Implemented and tested
- A complete document for any input, including the implicit html/head/body of empty input
- Elements and attributes (order kept; duplicate attribute names dropped by the parser)
- Character references in text and attribute values
- Adjacent text coalesced into one text node
- Foster parenting of text inside tables
- SVG and MathML elements keep their namespace
- `<script>` text kept as raw text; nothing is executed
- `<noscript>` parsed as markup (scripting is disabled)
- Quirks-mode flag recorded from the doctype
- UTF-8 input, with invalid bytes replaced by U+FFFD
- The resulting tree does not depend on how the input is split into chunks
- Limits fail closed with an error and no partial document: input size,
  node count, nesting depth, attributes per element, total string bytes

### Partially implemented
- `<template>`: its contents are stored as children of the `<template>` element
  (the DOM standard uses a separate fragment)
- Attribute namespaces: a prefixed attribute is stored as `prefix:local`
- Misnested formatting elements: only the order of the text is tested,
  not the exact tree shape

### Not represented or unsupported
- Comments, doctype nodes and processing instructions are discarded
- Encodings other than UTF-8; `<meta charset>` is ignored
- Form owner association
- Fragment parsing, `iframe srcdoc`, `document.write`
- Script execution

### Planned, not implemented
- CSS parsing, style, layout, painting, networking, JavaScript (see the roadmap)

## Provisional limits (defaults, configurable, revise only with evidence)

| Limit | Default | Applies to |
|---|---|---|
| Input size | 4 MiB | one `parse` call |
| DOM nodes | 50,000 | one document (detached nodes count) |
| Nesting depth | 256 | one document |
| String bytes (tags, text, attributes) | 4 MiB | one document |
| Attributes per element | 256 | one element |