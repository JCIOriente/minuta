---
description: LinkedIn post about the recording
input: transcript
output: fields
fields: [body, hashtags]
limits:
  body: { max_chars: 2800 }
  hashtags: { max: 5, max_chars: 150 }
format: plain
---
Write a LinkedIn post about "{{job_name}}" from the transcript in the input.

- body: the post. The first line is a hook that makes people keep reading. Use short paragraphs of one to three sentences, separated by blank lines. Share the main insight or lesson, in a professional but personal tone. Do not include links.
- hashtags: a few relevant hashtags.

Use only facts that are in the input. Do not invent names, numbers or claims.
