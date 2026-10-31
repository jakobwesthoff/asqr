---
title: "Set up Claude Code"
tags: [guide/agents]
site:
  order: 2
---
This page sets asqr up with Claude Code and follows one session from the
request to the result. The last section lists where other agents keep
skills.

## Steps

1. **Install asqr** as described on the page about the
   [first session](01M68ASRJ527YVE41S9V50YXQW-install-and-a-first-session.md).
2. **Give Claude Code the skill, or leave it out.** An agent you tell to
   use asqr reads the skill from the binary with `asqr skill`, so in most
   cases nothing needs installing. To install a copy, run this in your
   project:

   ```sh
   asqr skill --install .claude/skills
   ```

   It writes `asqr/SKILL.md` and its parts in `asqr/references/` under the
   skills directory you name. `.claude/skills` in a project and
   `~/.claude/skills` for every project are skills directories of Claude
   Code. Running the install again replaces the copy, parts included.
   An installed copy stays as it is when asqr is updated. `asqr ask`
   and `asqr validate` print a hint when they find a copy older than asqr.
3. **Start asqr and Claude Code in one tmux session.** Open a tmux session,
   run `asqr` in one pane and `claude` in a second pane of the same tmux
   session. The agents and asqr of one tmux session use the same queue
   without any flag. Outside tmux both use the default queue, which works
   as well.
4. **Ask the agent to use asqr.** The skill acts only on an explicit
   request, so say it, for example "put the questions about the release
   through asqr".
5. **Answer.** The session shows up in asqr with a notification and a bell.
6. **The result comes back** to the agent, which reads it and goes on.

## An example

You tell the agent: "Put the questions about the release through asqr."

In the format the skill describes, the agent writes the questions as a
session file and asks. This is a session it could write, as
`questions.json`:

```json
{
  "asqr": 2,
  "title": "Release 0.2.0",
  "intro": "Three decisions before the release. Skip what you cannot decide `yet`.",
  "from": "release script",
  "questions": [
    {
      "id": "go",
      "header": "Go?",
      "text": "All checks passed. Publish **0.2.0** now?",
      "kind": "single",
      "options": [
        { "id": "yes", "label": "Publish now" },
        { "id": "later", "label": "Later", "description": "Keep the tag, publish after the weekend." },
        { "id": "no", "label": "Do not publish", "description": "Delete the tag again." }
      ]
    },
    {
      "id": "channels",
      "header": "Channels",
      "text": "Where should the release be announced?",
      "kind": "multi",
      "max": 3,
      "options": [
        { "id": "changelog", "label": "Changelog", "default": true },
        { "id": "blog", "label": "Blog post" },
        { "id": "social", "label": "Social media" }
      ],
      "custom": { "label": "Somewhere else" }
    },
    {
      "id": "highlights",
      "header": "Highlights",
      "text": "One sentence for the release notes:",
      "kind": "text",
      "length": { "target": [40, 120], "warn": 140 }
    }
  ]
}
```

The agent's own questions will differ. The file has no `id`, as the skill
tells agents to leave it out. Then the agent asks and waits with the
commands the skill prescribes:

```sh
id=$(asqr ask questions.json)
asqr wait "$id" --timeout 540
```

In asqr, the first question of this session looks like this:

![A question of a session, with the title bar, the header, the tabs, the options and the key bar](../pictures/first-question.png)

You pick "Publish now", leave the channels as they are, type a sentence and
reach the review:

![The review tab of an answered session: every answer on a line, the counts and the submit button](../pictures/review.png)

`enter` on `[ Submit ]` sends the answers. `asqr wait` prints the result
and exits with 0. This is the result of a real run of this session. The id
and the time differ from run to run:

```json
{
  "asqr": 2,
  "id": "01M68C6004X3C3SK4S5X9RYNBS",
  "status": "submitted",
  "submitted_at": "2026-10-31T08:01:09+01:00",
  "session_sha256": "0b7af853270911da0703acd0ad93371681c078f649738c6a1d852b3590e83813",
  "answers": [
    {
      "question": "go",
      "selected": [
        "yes"
      ]
    },
    {
      "question": "channels",
      "selected": [
        "changelog"
      ],
      "defaulted": true
    },
    {
      "question": "highlights",
      "custom": "Startup is twice as fast."
    }
  ]
}
```

`defaulted: true` says that you left the default of the channels as it
was. The `custom` text is your typed sentence.

## Other agents

asqr looks for installed copies of the skill in the skills directories of
Claude Code, Codex, Gemini CLI, GitHub Copilot and Cursor: `.agents/skills`,
`.claude/skills`, `.gemini/skills`, `.github/skills`, `.cursor/skills` and
`.codex/skills` in the project, searched up to the git root, and
`~/.agents/skills`, `~/.claude/skills`, `~/.gemini/skills`,
`~/.copilot/skills`, `~/.cursor/skills` and `~/.codex/skills` in your home
directory. `asqr skill --install <directory>` writes the skill under the
directory you name.
