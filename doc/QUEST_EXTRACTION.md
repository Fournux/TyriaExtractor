# Guild Wars Quest Extraction

Quest text is stored in localized DAT resources, while quest identity, state,
objective order, map relations, dialogue roles, and completion evidence are
supplied at runtime. A reproducible catalog therefore requires a first-party
join rather than treating either source as complete by itself:

```text
runtime quest packets
  -> quest ID, EncStrings, objective variants, maps, and lifecycle

runtime NPC and dialogue packets
  -> observed giver, progress, and reward-NPC relations

quest EncStrings
  -> text references and compact-record seeds
  -> per-language DAT text resources
  -> localized quest fields, steps, rewards, and NPC labels
```

Archive addressing and text records are specified in
[GWDAT_FORMAT.md](GWDAT_FORMAT.md). Runtime item identity used for reward joins
is specified in [ITEM_EXTRACTION.md](ITEM_EXTRACTION.md). NPC identity and name
resolution are specified in
[NPC_AND_VENDOR_EXTRACTION.md](NPC_AND_VENDOR_EXTRACTION.md).

## 1. Source boundary

The confirmed source split is:

| Data | Source |
|---|---|
| Quest ID and current quest-log state | Runtime quest packets |
| Location/category, quest name, named NPC, description, and objective EncStrings | Runtime quest packets |
| Objective ordering and observed variants | Runtime objective packets |
| Origin map | `map_from` in quest metadata packets |
| Current marker and target map | Marker packets; mutable runtime state |
| Observed giver/progress/reward NPC | Dialogue sender/button packets joined to agent and map packets |
| Experience, gold, item-name, and skill-name reward terms | Structured segments inside the description EncString |
| Item reward model/file identity | Conservative join to runtime item observations |
| Localized text | Language-specific text resources in `Gw.dat` or a snapshot |

No complete static `quest_id -> metadata/text references` inventory has been
confirmed in the archive or client executable. The official quest-info request
also requires a quest already known to the active quest log, so it is not an
arbitrary quest-ID enumerator.

## 2. Quest packet layouts

The confirmed packet sizes include the four-byte header. Integer fields and
UTF-16 words are little-endian; offsets are relative to the packet start.

| Header | Bytes | Relevant content |
|---:|---:|---|
| `0x0049 QUEST_ADD` | `0x50` | Quest ID, marker/state fields, three eight-word EncStrings, origin map |
| `0x004c QUEST_DESCRIPTION` | `0x208` | Quest ID, 128-word description EncString, 128-word objectives EncString |
| `0x0050 QUEST_GENERAL_INFO` | `0x40` | Quest ID, state, three eight-word EncStrings, origin map |
| `0x0051 QUEST_UPDATE_MARKER` | `0x18` | Quest ID and current marker |
| `0x0052 QUEST_REMOVE` | `0x08` | Quest ID |
| `0x0053 QUEST_ADD_MARKER` | `0x18` | Quest ID and current marker |
| `0x0054 QUEST_UPDATE_OBJECTIVES` | `0x108` | Quest ID and 128-word objectives EncString |

A quest ID is at `+0x04` in every listed quest packet. The validated runtime
range is `1..=65535`; values outside that range are rejected as false handler
candidates rather than accepted as new quests.

### `QUEST_ADD`

| Offset | Type | Meaning |
|---:|---|---|
| `0x04` | `u32` | Quest ID |
| `0x1c` | `u16[8]` | Location/category EncString |
| `0x2c` | `u16[8]` | Quest-name EncString |
| `0x3c` | `u16[8]` | Named-NPC EncString |
| `0x4c` | `u32` | `map_from`, the observed origin map |

### `QUEST_DESCRIPTION`

| Offset | Type | Meaning |
|---:|---|---|
| `0x04` | `u32` | Quest ID |
| `0x08` | `u16[128]` | Description and reward EncString |
| `0x108` | `u16[128]` | Objectives EncString |

### `QUEST_GENERAL_INFO`

| Offset | Type | Meaning |
|---:|---|---|
| `0x04` | `u32` | Quest ID |
| `0x0c` | `u16[8]` | Location/category EncString |
| `0x1c` | `u16[8]` | Quest-name EncString |
| `0x2c` | `u16[8]` | Named-NPC EncString |
| `0x3c` | `u32` | `map_from`, the observed origin map |

### `QUEST_UPDATE_OBJECTIVES`

| Offset | Type | Meaning |
|---:|---|---|
| `0x04` | `u32` | Quest ID |
| `0x08` | `u16[128]` | Objectives EncString |

`map_from` is an origin observation. A marker packet's `map_to`, coordinates,
and plane describe the current mutable objective marker and must not replace the
origin map. One quest can emit several target maps during its lifecycle.

## 3. Completing active-quest evidence

Injecting or attaching an observer after login can miss descriptions for quests
that were already active. The official client exposes a quest-info request that
causes the normal `QUEST_DESCRIPTION` response for a known active quest.

A minimal complete capture strategy is:

1. Observe a quest ID through `QUEST_ADD`, `QUEST_GENERAL_INFO`, either marker
   packet, or `QUEST_UPDATE_OBJECTIVES`.
2. Request quest information once for that ID through the official client path.
3. Capture the resulting `QUEST_DESCRIPTION` through the normal packet handler.
4. Require one description observation for every quest included in a complete
   catalog.

The request does not manufacture text or enumerate unknown quests. It closes an
observer-timing gap only for quests already present in the client state.

## 4. Accumulation and conflict semantics

Accumulate quest evidence by `quest_id` across ordered sessions:

- retain every nonzero observed `map_from` value;
- keep the latest nonempty static EncString fields;
- retain every distinct objective step;
- retain every distinct ordered objective sequence;
- preserve dialogue-role observations separately from the quest packet's named
  NPC field;
- treat a missing `QUEST_DESCRIPTION` as incomplete evidence.

Objective sequences are observed variants. They are not proof of a canonical
linear order: quests can branch, update partially, or expose mutually exclusive
states. Flattening every observed step into one claimed sequence loses that
boundary.

## 5. Localized quest text from DAT resources

Runtime EncStrings provide text IDs, compact-record seeds, composition wrappers,
and ordering. `Gw.dat` or a snapshot supplies the localized records.

For each referenced text ID:

```text
file_index   = text_id / 1024
record_index = text_id % 1024
resource_id  = language_file_array[file_index]
```

Resolve the resource through the DAT hash table, decode the selected text record
with its captured seed, and repeat with every language array. Preserve raw
EncStrings and unresolved references; do not substitute a similarly numbered
record without matching first-party provenance.

Quest fields have distinct meanings even when their text happens to match:

- the location/category field labels the quest grouping or location;
- the named-NPC field is authored quest text;
- an observed dialogue NPC is a runtime relation.

The named-NPC field is not a giver identity. A quest can name one character in
its text while its observed giver and reward NPC are different models.

## 6. Objective steps

The objectives EncString contains segments separated by word `0x0002`. For each
nonempty segment:

1. If tag `0x010a` occurs, begin the content after that tag.
2. Decode the first text reference from the content.
3. Preserve both the reference and the complete segment so localized rendering
   retains its composition context.

Observed wrapper text IDs `10741` and `10742` represent completed/pending step
presentation. When rendering the step itself, treat the wrapper as state markup
and resolve the following content references. Preserve the raw wrapper because
additional objective-state semantics may exist.

## 7. Dialogue and NPC roles

Dialogue attribution uses these additional packets:

| Header | Bytes | Relevant fields |
|---:|---:|---|
| `0x0020 AGENT_SPAWNED` | `0x74` | Agent ID, NPC model composite, position |
| `0x0021 AGENT_DESPAWNED` | `0x08` | Agent ID |
| `0x0056 NPC_UPDATE_PROPERTIES` | `0x34` | NPC model ID and model-level name EncString |
| `0x007e DIALOG_BUTTON` | `0x110` | Button text and quest-dialog ID |
| `0x0081 DIALOG_SENDER` | `0x08` | Current sender agent ID |
| `0x0199 INSTANCE_LOAD_INFO` | `0x1c` | Current map ID |

`DIALOG_BUTTON.dialog_id` is at `+0x108`. A quest dialogue has bit
`0x00800000` set:

```text
quest_id   = (dialog_id ^ 0x00800000) >> 8
dialog_type = dialog_id & 0x0000000f
```

Confirmed low-nibble actions are:

| Value | Action | Catalog role |
|---:|---|---|
| `1` | Take | Giver |
| `2` | Decline | No positive role relation |
| `3` | Enquire | Giver |
| `4` | Enquire next | Progress NPC |
| `5` | Recap | Progress NPC |
| `6` | Enquire reward | Reward NPC |
| `7` | Reward | Reward NPC and completion candidate |

Join the current dialogue sender through `(session, agent_id)` to the live NPC
model and current map. Clear sender and agent state on despawn or map load.
Resolve the NPC model's captured name EncString through DAT text resources.
Transient agent IDs are evidence for the join, not stable catalog identifiers.

## 8. Structured rewards

Reward components are embedded in description segments separated by `0x0002`.
The first text reference identifies the reward kind:

| Text ID | Meaning | Value source |
|---:|---|---|
| `10728` | Reward section wrapper | Structural only |
| `10730` | Experience | Encoded integer following tag `0x0101` |
| `10732` | Gold | Encoded integer following tag `0x0101` |
| `10735` | Item | Following seeded item-name text reference |
| `10738` | Skill | Text ID encoded after tag `0x010a` |

A skill reward identifies localized skill text in the confirmed form; it does
not by itself prove a numeric skill ID.

An item reward EncString supplies an exact `(text_id, seed)` name reference but
no item model ID. A model join is safe only when:

1. that exact reference maps to one runtime-observed item model globally; or
2. a reward-button observation is followed by `QUEST_REMOVE` for the same quest
   and session, and exactly one matching item model is observed near that
   completion.

The validated completion guard accepts `QUEST_REMOVE` within 30 minutes of the
reward button. Temporal item correlation then uses a two-minute window around
the confirmed completion. If multiple models remain, preserve the localized
reward name without a model join.

Reward quantities, stat payloads, choice groups, and mutually exclusive rewards
are not established by the confirmed structure and must not be invented.

## 9. Capture ordering and validation

When observations come from several domain streams, retain a session identifier
and one monotonic sequence number shared by all streams. Merge rows by
`(session_id, sequence)` before reconstructing maps, agents, dialogues, and
vendor or item joins. File order alone is not sufficient.

A trustworthy capture must establish:

- the packet schema and exact expected size for every consumed family;
- no dropped rows, capacity evictions, or write failures;
- unique, gap-free sequence numbers for the supplied evidence;
- consistent client-build metadata within a session;
- complete description evidence for every cataloged quest.

Malformed packet sizes, header mismatches, impossible quest IDs, and conflicting
session metadata invalidate the affected evidence instead of producing partial
semantic claims.

## 10. Limits and open questions

- Runtime capture cannot prove exhaustive quest coverage without a confirmed
  finite first-party inventory.
- An active-quest info request cannot enumerate quests absent from the active
  log.
- Dialogue availability proves only that one action was available to one
  character at that moment; it does not establish generic prerequisites.
- Minimum levels, prerequisite quests, profession restrictions, and availability
  expressions remain unconfirmed.
- Objective variants are observations, not a complete branch graph.
- Item quantities, stats, and reward-choice semantics require additional
  first-party structure or controlled captures.
