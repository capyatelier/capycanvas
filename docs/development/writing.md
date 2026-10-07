# Writing docs and UI text

[Developer guide](README.md)

## Where each kind of text belongs

| Text | Where | Lifetime |
| --- | --- | --- |
| Rules for all work | [`AGENTS.md`](../../AGENTS.md) | Permanent; short |
| Setup, build, test and measurement how-tos | `docs/development/` | Current |
| How the code works, contracts and formats | `docs/architecture.md`, `internals/`, `reference/`, `ui/` | Current |
| Performance targets and results | [`PERFORMANCE_TARGETS.md`](../PERFORMANCE_TARGETS.md) and the tier tables in `performance/` | Current numbers only |
| A plan for work in progress | `docs/development/<plan>.md` | Deleted when the work lands |
| Research and design decisions | `docs/history/` | Kept; claims apply to when written |
| Progress, handoffs, validation reports, investigations, logs, measurement dumps, port inventories | Not committed: `artifacts/` or `*.local.md` | Local |

- Update the current guide in the same commit as the behaviour it describes.
  A guide that disagrees with the code is a bug; the code wins.
- When a plan's work lands, move its lasting decisions into the guides and delete
  the plan. Results go in commit messages and, for performance, the tier tables.
- A history record describes the design when it was written. Check the code
  before relying on it.
- State each rule once, in the guide that owns it, and link to it from elsewhere.

## Style

- Plain, concise, present tense. Explain what happens and why.
- Start a guide with a link to its parent index on line 3.
- Use tables for inventories and bold lead-ins for rule lists.
- Scope every claim to its evidence: name the device, build and date for a
  measurement, and say what was not verified. Keep measurements out of guides
  except the tier tables.
- Name devices by brand and model, such as Wacom MovinkPad 11, never by serial
  number.
- No session narration ("the user asked", "this session"), local paths, dates
  in file names, marketing tone or emojis.
- Commands, paths, flags and test names must exist. Check them when you write
  them and when you change what they refer to.

## README voice

The README explains why the people making Capy Canvas care about it. Its
subjects come from their experience, influences, opinions and ambitions. An
account of discovering digital drawing, doing comic work or seeing HDR photos
on a phone gives a feature a reason to enter the story. Starting with a feature
and inventing a personal reason for it reverses that process.

### Detecting a mismatch

| Ask | What to look for |
| --- | --- |
| Where did this subject come from? | A feature chosen to fill a section, with a generic benefit added afterward. Use an observation or intention the author actually supplied. |
| Could the sentences be shuffled without losing anything? | A list disguised as a paragraph. A story develops a thought; an inventory belongs in the feature table. |
| What does the author think about this? | Neutral descriptions with an occasional “we” added. Preserve the author's actual tastes and judgments. |
| Did we invent an experience? | An imagined sketchbook habit, favorite app or development anecdote presented as project history. Ask for missing history or leave it out. |
| Does this explain something the reader needs explained? | Definitions of familiar art tools, descriptions already visible in the screenshot, or details included only because they are implemented. |
| Is this sentence here to sell or reassure? | Generic promises, repeated invitations to try the app, or praise for ordinary behavior. |
| Are we manufacturing informality? | Deliberate errors, added slang, forced fragments or equal-length paragraphs with the same tidy ending. |

### Revising the README

- Preserve passages the author has marked as finished, including their spelling
  and punctuation. Edit only the agreed sections.
- Carry forward the author's subjects and connections. The engineering section
  should explain how we pursue the ambitions introduced earlier; contributing
  should give people a way to participate in the community described there.
- Name influences when the author names them. Keep personal judgments personal;
  check technical, historical and comparative claims separately. A claim doesn't
  become established because it makes a good story.
- Let the point determine the length. A thought can take one sentence or several
  paragraphs. Don't add a moral, benefit or summary to finish every paragraph.
- Preserve natural repetition, asides and changes of pace. Rough grammar isn't
  what makes the voice personal. Strong words such as “magic” can express a real
  reaction; a mechanical word blacklist cannot judge that.
- Keep inventories, setup and license text factual and brief. They don't need an
  origin story. Link detailed mechanisms to the engineering whitepaper.

These checks apply to README prose. The manual and UI text have different jobs.

## UI text

Write for a painter, not an engineer, and for every platform.

- Say what the artist can't rely on, why in their terms, and the one thing that
  fixes it. Say nothing when nothing needs fixing.
- Use one word per thing and only terms the app already teaches. Show a
  meaningful value, such as a colour profile and depth, not decoder or GPU
  terminology.
- Name real places, such as the operating system's display settings. Say "your
  operating system" when it is the one acting, and "Capy Canvas can't tell" when
  the app lacks the information, so each sentence is true on every platform.
- Scope each claim to the current screen and hedge only facts the app cannot know.
- No metaphors, personification, invented tasks or reassurance nobody asked for.
- Use one term per operation (Grow and Shrink, not Expand and Contract); other
  words can be search aliases. Name things by their visible names, such as "Hide
  Tool Set panel".
- End a command that opens a dialog or prompt with "…". Tooltips read
  "Label (Shortcut)".
- An unavailable command gives its specific reason, not a generic one.
- Don't add settings, menu items, placeholder items for missing features, or
  instructional hint text in panels and dialogs to solve a problem that better
  behaviour would solve.

Menu labels, command names and availability text are defined in shared Rust, so
every host shows the same words. Catalog messages use semantic identities and
complete sentences with named arguments; keep user text literal. Follow the
[localization guide](../ui/localization.md) when adding or translating shared
copy.

## Handoffs

A handoff prompt for another session states the goal, the key pointers into the
docs and code, the constraints and how to tell the work is done. Keep it short,
don't restate what the docs already say, and name the tablet by brand and
model or by tier. Keep it in the prompt or a `*.local.md` file, not in a commit.
