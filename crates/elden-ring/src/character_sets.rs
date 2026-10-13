//! Bounded ChrSet entries and opaque character identifiers; no engine reference pinning.
use crate::objects::{GameObjectCatalog, ObjectKind, ObjectObservation};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
use std::collections::BTreeSet;

pub(crate) const SET_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x492710,
        length: 0x104,
    },
    CodeRange {
        rva: 0x493d50,
        length: 0x4,
    },
    CodeRange {
        rva: 0x493d40,
        length: 0xf,
    },
    CodeRange {
        rva: 0x493ce0,
        length: 0x13,
    },
    CodeRange {
        rva: 0x493cb0,
        length: 0xc,
    },
    CodeRange {
        rva: 0x4955a0,
        length: 0xdd,
    },
    CodeRange {
        rva: 0x5092a0,
        length: 0x86,
    },
    CodeRange {
        rva: 0x4935e0,
        length: 0xba,
    },
    CodeRange {
        rva: 0x404080,
        length: 0x14a,
    },
    CodeRange {
        rva: 0x403350,
        length: 0x86,
    },
    CodeRange {
        rva: 0x504c40,
        length: 0x1372,
    },
    CodeRange {
        rva: 0x4ec3b0,
        length: 0xa4,
    },
    CodeRange {
        rva: 0x50bc40,
        length: 0x49a,
    },
    CodeRange {
        rva: 0x50e370,
        length: 0xea,
    },
    CodeRange {
        rva: 0x508b80,
        length: 0xe9,
    },
    CodeRange {
        rva: 0x3c3810,
        length: 0x11,
    },
    CodeRange {
        rva: 0x3f0700,
        length: 0x1ce,
    },
    CodeRange {
        rva: 0x3f2d30,
        length: 0x55,
    },
];
pub(crate) const SET_COUNT_OFFSET: u32 = 0x10;
pub(crate) const SET_ENTRIES_OFFSET: u32 = 0x18;
pub(crate) const SET_ENTRY_STRIDE: u32 = 0x10;
pub(crate) const CHARACTER_IDENTIFIER_OFFSET: u32 = 0x8;
pub(crate) const WORLD_PLAYER_SET_OFFSET: u32 = 0x10ee0;
pub(crate) const WORLD_CREATION_SET_OFFSET: u32 = 0x10fe8;
pub(crate) const CHARACTER_ENTRY_LINK_OFFSET: u32 = 0x10;
pub(crate) const WORLD_SECOND_SET_OFFSET: u32 = 0x10f38;
pub(crate) const WORLD_THIRD_SET_OFFSET: u32 = 0x10f90;
pub(crate) const WORLD_OPEN_FIELD_SET_OFFSET: u32 = 0x11040;
pub(crate) const WORLD_INDEXED_SETS_OFFSET: u32 = 0x1ded8;
pub(crate) const WORLD_INDEXED_SET_COUNT: u32 = 197;
const ENTRY_STATE_BYTES_OFFSET: u32 = 8;

/// Proven storage selections. The two middle embedded collections deliberately
/// carry their storage names until their gameplay roles are independently proved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldCollectionSelection {
    Players,
    Embedded10f38,
    Embedded10f90,
    Creation,
    OpenField,
    /// All native slots 0..197, including the last slot and aliased embedded sets.
    /// This is not a handle's collection ID or a network-role classification.
    Indexed(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldCollectionObservation {
    pub world_address: u64,
    pub epoch: u64,
    pub selection: WorldCollectionSelection,
    pub kind: ObjectKind,
    pub set: CharacterSetObservation,
}

/// Three original entry bytes, not a guessed load-status enum or a native lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldMemberStateObservation {
    pub registration: CharacterRegistrationObservation,
    pub selection: WorldCollectionSelection,
    pub set_kind: ObjectKind,
    /// Entry +8/+9/+a. Unknown state/request values and flag bits are preserved.
    pub native_state_bytes: [u8; 3],
}
impl WorldMemberStateObservation {
    /// Exact storage predicate used in native 3f0700. That function additionally
    /// tests entry flags, controller policy, character flags and update group.
    /// This predicate alone proves neither active behavior nor resource readiness.
    pub fn entry_state_is_four(&self) -> bool {
        self.native_state_bytes[0] == 4
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterEntryObservation {
    pub ordinal: u32,
    pub entry_address: u64,
    pub character: ObjectObservation,
    /// Opaque copied native +8 identifier. No inferred generation, role or block semantics.
    pub native_identifier: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterSetObservation {
    pub address: u64,
    pub capacity: u32,
    pub entries_address: u64,
    /// Only non-null entries; ordinals preserve holes in the native array.
    pub characters: Vec<CharacterEntryObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldPlayerObservation {
    pub world_address: u64,
    pub set: CharacterSetObservation,
}
/// Exact embedded set passed by the native debug-creation path. A copied member is not
/// an owned allocation or an engine lifetime/reference guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldCreationSetObservation {
    pub world_address: u64,
    pub set: CharacterSetObservation,
}

/// Copied identity for detecting visible target transitions. It is not an engine lease,
/// generation decoder or proof of registration in this world. Caller advances epoch on reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterIdentity {
    pub address: u64,
    pub world_address: u64,
    pub native_identifier: u64,
    pub epoch: u64,
}

/// Copied agreement between a known exact ChrSet member and its character's entry link.
/// This is a registration observation, not a reference count, pin, or destruction permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterRegistrationObservation {
    pub character: CharacterIdentity,
    pub set_address: u64,
    pub entry_address: u64,
    pub ordinal: u32,
}

/// The character's reciprocal entry link and three selected raw state bytes.
/// No set membership, ordinal, lifecycle state or engine reference is inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterEntryStateObservation {
    pub character: CharacterIdentity,
    pub entry_address: u64,
    pub native_state_bytes: [u8; 3],
}

fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy only the entry pointer, its reciprocal character pointer and bytes8/9/a.
    /// The complete native3f2d30 uses bytea bit0 as one rejection guard; the other
    /// native guards are deliberately not evaluated by this raw observation.
    /// No collection scan, native function, ownership acquisition or write occurs.
    pub fn character_entry_state(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterEntryStateObservation>> {
        if target.epoch == 0 {
            return Err(Error::InvalidProfile("zero character entry state epoch"));
        }
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile(
                "stale character entry state identity",
            ));
        }
        let memory = self.module.memory();
        let link = at(target.address, CHARACTER_ENTRY_LINK_OFFSET)?;
        let entry_address = memory.read_u64(link)?;
        if entry_address == 0 {
            if memory.read_u64(link)? != 0
                || self.character_identity(target.address, target.epoch)? != Some(target)
            {
                return Err(Error::InvalidProfile("character entry link appeared"));
            }
            return Ok(None);
        }
        if !entry_address.is_multiple_of(8) {
            return Err(Error::InvalidProfile("unaligned character entry state"));
        }
        if memory.read_u64(entry_address)? != target.address {
            return Err(Error::InvalidProfile(
                "character entry backpointer disagrees",
            ));
        }
        let address = at(entry_address, ENTRY_STATE_BYTES_OFFSET)?;
        let mut native_state_bytes = [0; 3];
        memory.read_exact(address, &mut native_state_bytes)?;
        let mut check = [0; 3];
        memory.read_exact(address, &mut check)?;
        if check != native_state_bytes
            || memory.read_u64(entry_address)? != target.address
            || memory.read_u64(link)? != entry_address
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile("character entry state changed"));
        }
        Ok(Some(CharacterEntryStateObservation {
            character: target,
            entry_address,
            native_state_bytes,
        }))
    }

    /// Resolve the supplied identity in one caller-selected exact ChrSet, preserving holes.
    /// Missing target/set/membership returns None; inconsistent links and detectable changes fail.
    /// Does not decode a handle, enumerate OpenFieldChrSet, establish the set's world ownership,
    /// register/unregister a character, or keep either allocation alive.
    pub fn character_registration(
        &self,
        target: CharacterIdentity,
        set_address: u64,
        limit: usize,
    ) -> Result<Option<CharacterRegistrationObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile(
                "stale character registration identity",
            ));
        }
        let Some(set) = self.character_set(set_address, limit)? else {
            return Ok(None);
        };
        let Some(entry) = set
            .characters
            .iter()
            .find(|entry| entry.character.address == target.address)
        else {
            return Ok(None);
        };
        let slot = at(target.address, CHARACTER_ENTRY_LINK_OFFSET)?;
        if entry.native_identifier != target.native_identifier
            || self.module.memory().read_u64(slot)? != entry.entry_address
        {
            return Err(Error::InvalidProfile("character and set entry disagree"));
        }
        let result = CharacterRegistrationObservation {
            character: target,
            set_address,
            entry_address: entry.entry_address,
            ordinal: entry.ordinal,
        };
        if self.character_set(set_address, limit)? != Some(set)
            || self.module.memory().read_u64(slot)? != result.entry_address
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "character registration changed during observation",
            ));
        }
        Ok(Some(result))
    }

    pub fn character_identity(
        &self,
        address: u64,
        epoch: u64,
    ) -> Result<Option<CharacterIdentity>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("character epoch must be nonzero"));
        }
        let Some(world) = self.world()? else {
            return Ok(None);
        };
        let Some(character) = self.character(address)? else {
            return Ok(None);
        };
        let identifier_slot = at(address, CHARACTER_IDENTIFIER_OFFSET)?;
        let native_identifier = self.module.memory().read_u64(identifier_slot)?;
        if self.module.memory().read_u64(identifier_slot)? != native_identifier
            || self.character(address)? != Some(character)
            || self.world()? != Some(world)
        {
            return Err(Error::InvalidProfile(
                "character identity changed during observation",
            ));
        }
        Ok(Some(CharacterIdentity {
            address,
            world_address: world.address,
            native_identifier,
            epoch,
        }))
    }

    pub fn primary_player_identity(&self, epoch: u64) -> Result<Option<CharacterIdentity>> {
        let Some(player) = self.primary_player()? else {
            return Ok(None);
        };
        let identity = self.character_identity(player.address, epoch)?;
        if self.primary_player()? != Some(player) {
            return Err(Error::InvalidProfile(
                "primary player changed during identity observation",
            ));
        }
        Ok(identity)
    }

    /// Copies bounded non-null entries from the exact ChrSet primary type. OpenFieldChrSet
    /// has different dispatch and is deliberately rejected. A snapshot is not a reference guard.
    pub fn character_set(
        &self,
        address: u64,
        limit: usize,
    ) -> Result<Option<CharacterSetObservation>> {
        self.copy_character_set(address, limit, ObjectKind::CharacterSet)
    }

    /// The inherited capacity/entry accessors are identical, while initialization
    /// and mutation dispatch differ. No ordinary ChrSet mutation is authorized.
    pub fn open_field_characters(
        &self,
        address: u64,
        limit: usize,
    ) -> Result<Option<CharacterSetObservation>> {
        self.copy_character_set(address, limit, ObjectKind::OpenFieldCharacterSet)
    }

    fn copy_character_set(
        &self,
        address: u64,
        limit: usize,
        expected_kind: ObjectKind,
    ) -> Result<Option<CharacterSetObservation>> {
        if !(1..=65536).contains(&limit) {
            return Err(Error::InvalidProfile("invalid character set limit"));
        }
        let Some(kind) = self.require(address, expected_kind)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let count_address = at(address, SET_COUNT_OFFSET)?;
        let entries_slot = at(address, SET_ENTRIES_OFFSET)?;
        let capacity = memory.read_u32(count_address)?;
        let entries = memory.read_u64(entries_slot)?;
        if capacity > i32::MAX as u32
            || capacity as usize > limit
            || !entries.is_multiple_of(8)
            || (capacity != 0 && entries == 0)
        {
            return Err(Error::InvalidProfile(
                "invalid character set capacity or storage",
            ));
        }
        let length = capacity as usize * SET_ENTRY_STRIDE as usize;
        entries
            .checked_add(length as u64)
            .ok_or(Error::AddressOverflow)?;
        // Only the first pointer is proven by the accessors. Do not sample or compare
        // the entry's other fields/padding merely because its stride is sixteen bytes.
        let mut pointers = Vec::with_capacity(capacity as usize);
        let mut characters = Vec::new();
        let mut seen = BTreeSet::new();
        for ordinal in 0..capacity {
            let entry_address = at(entries, ordinal * SET_ENTRY_STRIDE)?;
            let pointer = memory.read_u64(entry_address)?;
            pointers.push(pointer);
            if pointer == 0 {
                continue;
            }
            if !pointer.is_multiple_of(8) || !seen.insert(pointer) {
                return Err(Error::InvalidProfile(
                    "unaligned or duplicate character set pointer",
                ));
            }
            let character = self
                .character(pointer)?
                .ok_or(Error::InvalidProfile("character set pointer is absent"))?;
            let native_identifier = memory.read_u64(at(pointer, CHARACTER_IDENTIFIER_OFFSET)?)?;
            characters.push(CharacterEntryObservation {
                ordinal,
                entry_address,
                character,
                native_identifier,
            });
        }
        for (ordinal, pointer) in pointers.into_iter().enumerate() {
            if memory.read_u64(at(entries, ordinal as u32 * SET_ENTRY_STRIDE)?)? != pointer {
                return Err(Error::InvalidProfile(
                    "character set changed during observation",
                ));
            }
        }
        if memory.read_u32(count_address)? != capacity
            || memory.read_u64(entries_slot)? != entries
            || self.require(address, expected_kind)? != Some(kind)
        {
            return Err(Error::InvalidProfile(
                "character set changed during observation",
            ));
        }
        for entry in &characters {
            if self.character(entry.character.address)? != Some(entry.character)
                || memory.read_u64(at(entry.character.address, CHARACTER_IDENTIFIER_OFFSET)?)?
                    != entry.native_identifier
            {
                return Err(Error::InvalidProfile(
                    "character set member changed during observation",
                ));
            }
        }
        Ok(Some(CharacterSetObservation {
            address,
            capacity,
            entries_address: entries,
            characters,
        }))
    }

    /// Copy one exact current-world collection and verify every reciprocal entry.
    /// Missing world/indexed slot returns None; initialized empty embedded sets are
    /// Some(empty). Unknown derived types, changed payloads or ownership disagreement
    /// fail. Aliased slots are preserved; caller chooses deduplication and exclusions.
    /// This acquires no reference and does not classify active/hidden/ghost/network actors.
    pub fn world_collection(
        &self,
        selection: WorldCollectionSelection,
        epoch: u64,
        limit: usize,
    ) -> Result<Option<WorldCollectionObservation>> {
        if epoch == 0
            || !(1..=65536).contains(&limit)
            || matches!(selection, WorldCollectionSelection::Indexed(index)
                if u32::from(index) >= WORLD_INDEXED_SET_COUNT)
        {
            return Err(Error::InvalidProfile(
                "invalid world collection selection or bounds",
            ));
        }
        let Some(world) = self.world()? else {
            return Ok(None);
        };
        let (offset, embedded_kind) = match selection {
            WorldCollectionSelection::Players => {
                (WORLD_PLAYER_SET_OFFSET, Some(ObjectKind::CharacterSet))
            }
            WorldCollectionSelection::Embedded10f38 => {
                (WORLD_SECOND_SET_OFFSET, Some(ObjectKind::CharacterSet))
            }
            WorldCollectionSelection::Embedded10f90 => {
                (WORLD_THIRD_SET_OFFSET, Some(ObjectKind::CharacterSet))
            }
            WorldCollectionSelection::Creation => {
                (WORLD_CREATION_SET_OFFSET, Some(ObjectKind::CharacterSet))
            }
            WorldCollectionSelection::OpenField => (
                WORLD_OPEN_FIELD_SET_OFFSET,
                Some(ObjectKind::OpenFieldCharacterSet),
            ),
            WorldCollectionSelection::Indexed(index) => {
                (WORLD_INDEXED_SETS_OFFSET + u32::from(index) * 8, None)
            }
        };
        let slot = at(world.address, offset)?;
        let address = if embedded_kind.is_some() {
            slot
        } else {
            self.module.memory().read_u64(slot)?
        };
        let copied = if let Some(object) = self.identify(address)? {
            if embedded_kind.is_some_and(|kind| kind != object.kind)
                || !matches!(
                    object.kind,
                    ObjectKind::CharacterSet | ObjectKind::OpenFieldCharacterSet
                )
            {
                return Err(Error::InvalidProfile(
                    "foreign world collection primary type",
                ));
            }
            let set = self
                .copy_character_set(address, limit, object.kind)?
                .ok_or(Error::InvalidProfile("world collection vanished"))?;
            let reciprocal = || -> Result<()> {
                for member in &set.characters {
                    if self
                        .module
                        .memory()
                        .read_u64(at(member.character.address, CHARACTER_ENTRY_LINK_OFFSET)?)?
                        != member.entry_address
                    {
                        return Err(Error::InvalidProfile(
                            "world collection reciprocal entry disagrees",
                        ));
                    }
                }
                Ok(())
            };
            reciprocal()?;
            if self.copy_character_set(address, limit, object.kind)? != Some(set.clone()) {
                return Err(Error::InvalidProfile("world collection contents changed"));
            }
            reciprocal()?;
            Some(WorldCollectionObservation {
                world_address: world.address,
                epoch,
                selection,
                kind: object.kind,
                set,
            })
        } else {
            None
        };
        if self.world()? != Some(world)
            || (embedded_kind.is_none() && self.module.memory().read_u64(slot)? != address)
        {
            return Err(Error::InvalidProfile("world collection source changed"));
        }
        Ok(copied)
    }

    /// Observe a registered target's raw state only in a proven current-world set.
    /// A missing member is None; stale identity, conflicting membership or changed
    /// bytes fail. No state writes, lifecycle transition, retry or reference capture.
    pub fn world_member_state(
        &self,
        target: CharacterIdentity,
        selection: WorldCollectionSelection,
        limit: usize,
    ) -> Result<Option<WorldMemberStateObservation>> {
        let Some(collection) = self.world_collection(selection, target.epoch, limit)? else {
            return Ok(None);
        };
        if collection.world_address != target.world_address
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "foreign or stale world member state identity",
            ));
        }
        let Some(member) = collection
            .set
            .characters
            .iter()
            .find(|member| member.character.address == target.address)
        else {
            return Ok(None);
        };
        if member.native_identifier != target.native_identifier {
            return Err(Error::InvalidProfile(
                "world member state identifier disagrees",
            ));
        }
        let address = at(member.entry_address, ENTRY_STATE_BYTES_OFFSET)?;
        let mut bytes = [0; 3];
        self.module.memory().read_exact(address, &mut bytes)?;
        if self.world_collection(selection, target.epoch, limit)? != Some(collection.clone()) {
            return Err(Error::InvalidProfile(
                "world member state collection changed",
            ));
        }
        let mut check = [0; 3];
        self.module.memory().read_exact(address, &mut check)?;
        if check != bytes
            || self.character_identity(target.address, target.epoch)? != Some(target)
            || self
                .module
                .memory()
                .read_u64(at(target.address, CHARACTER_ENTRY_LINK_OFFSET)?)?
                != member.entry_address
        {
            return Err(Error::InvalidProfile("world member state source changed"));
        }
        Ok(Some(WorldMemberStateObservation {
            registration: CharacterRegistrationObservation {
                character: target,
                set_address: collection.set.address,
                entry_address: member.entry_address,
                ordinal: member.ordinal,
            },
            selection,
            set_kind: collection.kind,
            native_state_bytes: bytes,
        }))
    }

    /// Native world's first embedded set, traversed by player lookup/count paths. Every
    /// non-null member must independently have the exact PlayerIns type. No network role implied.
    pub fn world_players(&self, limit: usize) -> Result<Option<WorldPlayerObservation>> {
        let Some(world) = self.world()? else {
            return Ok(None);
        };
        let set = self
            .character_set(at(world.address, WORLD_PLAYER_SET_OFFSET)?, limit)?
            .ok_or(Error::InvalidProfile("embedded player set is absent"))?;
        if set
            .characters
            .iter()
            .any(|entry| entry.character.kind != ObjectKind::Player)
        {
            return Err(Error::InvalidProfile(
                "player set contains a non-player type",
            ));
        }
        if self.world()? != Some(world) {
            return Err(Error::InvalidProfile(
                "world changed during player enumeration",
            ));
        }
        Ok(Some(WorldPlayerObservation {
            world_address: world.address,
            set,
        }))
    }
    /// Constructor504c40 initializes this exact ChrSet through492710. Native507b70
    /// passes it to its factory; no complete ABI for that protected entry is exported.
    /// Observe all copied entries twice; missing world returns None, mismatches fail.
    pub fn world_creation_set(&self, limit: usize) -> Result<Option<WorldCreationSetObservation>> {
        let Some(world) = self.world()? else {
            return Ok(None);
        };
        let set_address = at(world.address, WORLD_CREATION_SET_OFFSET)?;
        let set = self
            .character_set(set_address, limit)?
            .ok_or(Error::InvalidProfile("embedded creation set absent"))?;
        let check_links = || -> Result<()> {
            for entry in &set.characters {
                if self
                    .module
                    .memory()
                    .read_u64(at(entry.character.address, CHARACTER_ENTRY_LINK_OFFSET)?)?
                    != entry.entry_address
                {
                    return Err(Error::InvalidProfile(
                        "creation set member reciprocal link disagrees",
                    ));
                }
            }
            Ok(())
        };
        check_links()?;
        if self.character_set(set_address, limit)? != Some(set.clone())
            || self.world()? != Some(world)
        {
            return Err(Error::InvalidProfile(
                "world creation set changed during observation",
            ));
        }
        check_links()?;
        Ok(Some(WorldCreationSetObservation {
            world_address: world.address,
            set,
        }))
    }
}
