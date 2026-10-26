---
description: Timestamped chapters (YouTube rules)
input: transcript
output: chapters
limits:
  chapters: { max: 20, max_chars: 1500 }
---
Divide the recording "{{job_name}}" ({{duration}}) into chapters by topic, for the YouTube
description. Start a chapter where the speakers move to a new topic, not at every pause. Each
title names the topic in a few words that a viewer would recognize. Use only topics that are
in the input.
