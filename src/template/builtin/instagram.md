---
description: Instagram caption about the recording
input: summary
output: fields
fields: [body, hashtags]
limits:
  body: { max_chars: 1800 }
  hashtags: { max: 30, max_chars: 390 }
format: plain
---
Write an Instagram caption about "{{job_name}}" from the summary in the input.

- body: the caption. Open with a hook in the first line, since only that line shows before "more". Keep it casual and easy to read, in short paragraphs. End with an optional call to action, such as asking a question or inviting people to comment or save the post.
- hashtags: relevant hashtags, from broad to specific.

Use only facts that are in the input. Do not invent names, numbers or claims.
