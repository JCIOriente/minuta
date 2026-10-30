---
description: YouTube title, description, tags and thumbnail text
input: transcript
output: fields
fields: [title, body, chapters, tags, thumbnail_text]
limits:
  title: { max_chars: 100 }
  body: { max_chars: 3450 }
  tags: { max: 15, max_chars: 500 }
  thumbnail_text: { max_chars: 40 }
format: plain
---
Write the YouTube copy for a video called "{{job_name}}" that lasts {{duration}}. The input is the recording's transcript.

- title: clear and searchable. Put the main topic and its key words first.
- body: the video description. Its first two lines are the preview people see before they expand it, so make them say what the video is about and why to watch. Then cover the main points in short paragraphs.
- Do not write chapters or timestamps in the body. The chapter list is added after it.
- tags: search keywords people would type to find this video.
- thumbnail_text: a few punchy words for the thumbnail image.

Use only facts that are in the input. Do not invent names, numbers, links or claims.
