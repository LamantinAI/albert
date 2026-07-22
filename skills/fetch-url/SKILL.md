---
name: fetch-url
description: when the user wants the raw content or headers of a web page / URL
---
Fetch a URL from inside the sandbox and report what came back. Run it through the
forkd runner (dispatch to the "forkd" connector, kind `forkd.run`):

```
forkd.run {
  "interpreter": "bash",
  "script": "curl -sSL --max-time 20 -A 'albert/1.0' \"$1\"",
  "args": ["<the URL>"],
  "timeout_secs": 25
}
```

- The result's `stdout` is the page body. For headers only, use `curl -sSLI` instead.
- Summarise the content for the user in plain words — do NOT paste a large body
  verbatim; pull out the title / the parts they asked for.
- If `exit_code` is non-zero (or null with `timed_out: true`), report the problem
  from `stderr` honestly instead of inventing content.

Pass the URL as an arg (never interpolate it into the script string) so a odd URL
can't break the command.
