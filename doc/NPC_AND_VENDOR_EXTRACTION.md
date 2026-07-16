# Guild Wars NPC and Vendor Extraction

NPC definitions, displayed names, map presence, and service inventories do not
come from one static table. They are reconstructed by joining official-client
runtime observations with localized text and resource data from `Gw.dat` or a
snapshot:

```text
runtime NPC packets
  -> transient agent and stable NPC model relations
  -> model resources, gameplay metadata, map and position observations

runtime service-window state
  -> collector, merchant, crafter, and skill-trainer inventories
  -> transient owner agent

captured EncString
  -> text references and compact-record seeds
  -> per-language DAT text resources
  -> localized model or service-instance name
```

Archive addressing and text records are specified in
[GWDAT_FORMAT.md](GWDAT_FORMAT.md). Item identity and model-file resources are
specified in [ITEM_EXTRACTION.md](ITEM_EXTRACTION.md).

## 1. Source boundary

The validated source split is:

| Data | Source |
|---|---|
| NPC model/file relation, skin, visual adjustment, appearance, flags, profession, level | `NPC_UPDATE_PROPERTIES` runtime packet |
| NPC model composites | `NPC_UPDATE_MODEL` runtime packet, when observed |
| Agent lifetime, position, and NPC-model relation | spawn/despawn runtime packets |
| Map observation | instance-load runtime packet joined to the active session |
| Model-level NPC name EncString | `NPC_UPDATE_PROPERTIES` |
| Per-agent displayed name EncString | `AGENT_UPDATE_NPC_NAME` |
| Collector exchange | collector service-constructor state plus runtime item lookup |
| Merchant and crafter inventory | service-constructor item IDs plus runtime item lookup |
| Skill-trainer inventory | service-constructor skill entries |
| Localized strings | language-specific text resources in `Gw.dat` or a snapshot |
| Model and texture bytes | resources addressed by the observed file IDs |

No complete static NPC-definition or vendor-inventory table has been confirmed
in the archive or client executable. Absence of a literal packet tuple from the
archive does not prove that no encoded representation exists, but runtime
relations must not be replaced with guessed archive joins.

## 2. Identity layers

Several identifiers must remain distinct:

| Identifier | Scope and meaning |
|---|---|
| `agent_id` | Transient identity for one live agent. It is valid only within its session and lifetime. |
| `npc_model_id` | Reusable NPC archetype/model identity. Different displayed NPCs and services can share it. |
| `model_file_id` | DAT resource key supplied by the NPC definition. |
| `skin_file_id` | Additional DAT resource key supplied by the NPC definition. |
| Service-instance key | `(map_id, npc_model_id, position.x bits, position.y bits)` for one observed service NPC. |

An `agent_id` is used only to correlate packets. It must be discarded on
`AGENT_DESPAWNED` and all agent state must be reset on a map change.

A vendor must not be keyed by `npc_model_id` alone. The official client can
instantiate the same model at different positions with different names, roles,
and inventories. Floating-point position is compared by its exact packet bits
so identity does not depend on formatting or rounding.

## 3. NPC and context packets

The confirmed packet sizes include the four-byte header. All fields below are
little-endian, and offsets are relative to the packet start.

| Header | Bytes | Relevant fields |
|---:|---:|---|
| `0x0020 AGENT_SPAWNED` | `0x74` | `agent_id` at `+0x04`; composite agent type at `+0x08`; `x` / `y` as `f32` at `+0x14` / `+0x18` |
| `0x0021 AGENT_DESPAWNED` | `0x08` | `agent_id` at `+0x04` |
| `0x0056 NPC_UPDATE_PROPERTIES` | `0x34` | NPC definition described below |
| `0x0057 NPC_UPDATE_MODEL` | `0x2c` in the observed fixed-capacity form | `npc_model_id`, count, and up to eight model-file IDs |
| `0x009b AGENT_UPDATE_NPC_NAME` | `0x48` | `agent_id` at `+0x04`; displayed-name EncString as 32 UTF-16 words at `+0x08` |
| `0x00c3 WINDOW_MERCHANT` | `0x0c` | window transaction value at `+0x04` |
| `0x00c4 WINDOW_OWNER` | `0x08` | service-window owner `agent_id` at `+0x04` |
| `0x0199 INSTANCE_LOAD_INFO` | `0x1c` | current `map_id` at `+0x08` |

For `AGENT_SPAWNED`, an NPC composite has high nibble `0x2`; its stable model
identity is:

```text
is_npc       = (agent_type & 0xf0000000) == 0x20000000
npc_model_id = agent_type & 0x0fffffff
```

### `NPC_UPDATE_PROPERTIES`

| Offset | Type | Meaning |
|---:|---|---|
| `0x04` | `u32` | NPC model ID |
| `0x08` | `u32` | Model file ID |
| `0x0c` | `u32` | Skin file ID |
| `0x10` | `u32` | Packed visual adjustment |
| `0x14` | `u32` | Appearance value |
| `0x18` | `u32` | NPC flags |
| `0x1c` | `u32` | Primary profession |
| `0x20` | `u32` | Default level |
| `0x24` | `u16[8]` | Zero-terminated model-level name EncString |

The visual-adjustment dword contains four bytes in order: signed hue, signed
saturation, signed lightness, and unsigned scale percentage. Preserve the raw
dword in addition to any decoded view.

### `NPC_UPDATE_MODEL`

| Offset | Type | Meaning |
|---:|---|---|
| `0x04` | `u32` | NPC model ID |
| `0x08` | `u32` | Number of model-file IDs, at most eight in the confirmed form |
| `0x0c` | `u32[count]` | Model-file IDs |

Reject a count that exceeds the packet capacity. Preserve distinct composite
lists because more than one observation may be valid for one NPC model.

## 4. NPC reconstruction

Process observations in session order:

1. `INSTANCE_LOAD_INFO` establishes the current map and clears all transient
   agent state for that session.
2. `AGENT_SPAWNED` creates the `(session, agent_id) -> npc_model_id` relation and
   records the packet position for service-instance identity.
3. `NPC_UPDATE_PROPERTIES` supplies model-level metadata and the archetype name
   EncString keyed by `npc_model_id`.
4. `AGENT_UPDATE_NPC_NAME` supplies the displayed name keyed by transient
   `agent_id`. It may arrive before the spawn, so pending names must be retained
   until the agent relation is known.
5. `AGENT_DESPAWNED` removes the transient relation and pending name.

Conflicting complete definitions for one `npc_model_id`, a changed EncString for
one live agent, non-finite positions, malformed packet lengths, and truncated
composites are evidence errors rather than alternate catalog rows.

## 5. Localized names from DAT resources

The two name sources have different scopes:

- the eight-word name in `NPC_UPDATE_PROPERTIES` labels the reusable NPC model
  or archetype;
- the 32-word name in `AGENT_UPDATE_NPC_NAME` is the displayed name of one live
  service instance.

A model-level label must not replace a missing per-agent service name. Two agents
sharing one `npc_model_id` can display different names.

Decode each EncString into its text references and compact-record seeds. For
each text ID:

```text
file_index   = text_id / 1024
record_index = text_id % 1024
resource_id  = language_file_array[file_index]
```

Resolve `resource_id` through the DAT hash table, decode the selected plain or
compact text record, and repeat with each language array. Preserve the original
EncString and unresolved references; localized text must not be guessed from the
NPC model ID, file ID, archive proximity, or another NPC using the same model.

## 6. Service-window reconstruction

The owner relation begins with `WINDOW_OWNER.agent_id`. Join that agent through
the active spawn relation to obtain the service-instance key. The service code
inside the client constructor is authoritative:

| Service code | Meaning |
|---:|---|
| `1` | Merchant purchase list |
| `2` | Collector exchange |
| `3` | Crafter product list |
| `10` | Skill trainer |

`WINDOW_MERCHANT` is useful context but is not emitted for every collector flow
and therefore is not a required collector discriminator.

The runtime constructor layouts below were confirmed in an analyzed 32-bit
client build. Function addresses and signatures are build-specific and must be
rediscovered and validated after client updates.

### Collector

The collector create parameter contains:

| Offset | Type | Meaning |
|---:|---|---|
| `0x00` | `u32` | Service code; must be `2` |
| `0x14` | `u32` | Reward count |
| `0x18` | pointer | Reward runtime item-ID array |
| `0x20` | `u32` | Player trophy item ID |

After the official constructor completes, the collector object contains the
required quantity at `+0x0c` and required trophy model ID at `+0x10`. Each
reward runtime item ID is resolved through the official item manager to its
`model_id`, `model_file_id`, and item type.

A complete collector offer requires a nonzero required model and quantity, a
nonzero reward count, a readable non-truncated reward array, and resolved stable
fields for every reward. A trophy runtime item ID is not a stable catalog key.

### Merchant and crafter

Services `1` and `3` use the same create-parameter list shape:

| Offset | Type | Meaning |
|---:|---|---|
| `0x00` | `u32` | Service code |
| `0x14` | `u32` | Entry count |
| `0x18` | pointer | Array of runtime item IDs, stride four bytes |

Resolve every runtime item ID through the official item manager and retain its
stable `model_id`, `model_file_id`, item type, and base value. Reject the whole
observed list when it is unreadable, truncated, or contains unresolved entries;
a partial list is not a complete vendor inventory.

### Skill trainer

Service `10` uses:

| Offset | Type | Meaning |
|---:|---|---|
| `0x00` | `u32` | Service code |
| `0x10` | `u32` | Entry count |
| `0x14` | pointer | Array of eight-byte entries |

Each entry contains a skill ID followed by a second dword whose complete
semantics are not established. Preserve that dword as raw availability flags.
When repeated observations disagree, retain every distinct raw value for the
skill rather than inventing a boolean interpretation.

## 7. Catalog and coverage semantics

A normalized service catalog can safely retain:

- the service-instance key and per-agent localized name;
- collector required item, quantity, and resolved rewards;
- merchant/crafter model ID, model file ID, item type, and base value;
- trainer skill IDs and raw availability values;
- the set of observed service instances grouped by map.

Deduplicate exact repeated offers or entries, but never merge service instances
solely because they share an NPC model. A map coverage list is an observation
ledger, not proof that every outpost or service NPC has been visited.

## 8. Limits and open questions

- Runtime coverage is observational; no complete static NPC or service inventory
  is known.
- NPC flags alone do not establish collector, merchant, crafter, or trainer
  roles.
- Model and texture bytes can be read from the DAT once a file ID is known, but
  they do not establish the gameplay identity or service relation.
- Per-agent names require the display-name packet; a model-level name is not a
  safe fallback.
- The second skill-trainer entry dword remains raw.
- Vendor availability conditions, character-dependent inventories, and a
  complete outpost traversal set require additional first-party evidence.
