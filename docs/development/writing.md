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
- No session narration ("the user asked", "this session"), local paths, device
  serial numbers, dates in file names, marketing tone or emojis.
- Commands, paths, flags and test names must exist. Check them when you write
  them and when you change what they refer to.

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
every host shows the same words.

## Handoffs

A handoff prompt for another session states the goal, the key pointers into the
docs and code, the constraints and how to tell the work is done. Keep it short,
don't restate what the docs already say, and name a device tier rather than a
tablet. Keep it in the prompt or a `*.local.md` file, not in a commit.
