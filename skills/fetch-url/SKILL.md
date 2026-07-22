---
name: fetch-url
description: when the user wants the raw content or headers of a web page / URL
---
Fetch a URL from inside the sandbox and report what came back. This skill bundles
`scripts/fetch.sh` — run it IN PLACE through the forkd runner (dispatch to the
"forkd" connector, kind `forkd.run`); never copy the script anywhere:

```
forkd.run {
  "skill_path": "fetch-url/scripts/fetch.sh",
  "interpreter": "bash",
  "args": ["<the URL>"],
  "timeout_secs": 25
}
```

- The result's `stdout` is the page body. For headers only, add a second arg:
  `"args": ["<the URL>", "headers"]`.
- Summarise the content for the user in plain words — do NOT paste a large body
  verbatim; pull out the title / the parts they asked for.
- If `exit_code` is non-zero (or null with `timed_out: true`), report the problem
  from `stderr` honestly instead of inventing content.

Always pass the URL as an arg — never interpolate it into a script string.
