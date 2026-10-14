# Deploy checklist

I am about to deploy the new search page. A few things to confirm first.
See the [design doc](https://example.com/design) for context.

```askdown
id: target
type: choice
question: Which environment should I deploy to?
required: true
options:
  - staging
  - { label: Production, value: production }
```

```askdown
id: features
type: choice
question: Which feature flags should be on?
multiple: true
other: true
options:
  - new-ranking
  - spell-check
  - autocomplete
```

```askdown
id: note
type: text
question: Anything I should mention in the release note?
multiline: true
placeholder: Optional
```
