//! Experimental bounded copies of the native Think action-event list.
//! Keys remain raw. Predicate changes do not prove animation or action execution.
use crate::reader::{NightreignReader, ReaderCore};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
use std::collections::BTreeSet;
pub(crate) const AI_FUNC_TABLE: u32 = 0x2c016b0;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThinkActionEvent {
    pub address: u64,
    pub raw_key_a: u32,
    pub raw_key_b: u32,
    pub raw_flag: u8,
    pub raw_tag: u32,
}
/// Observed result of the native predicate sampler's last update.
/// Initial zero tags also map to NoChange; they do not prove an update occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkPredicateTransition {
    NoChange,
    BecameTrue,
    BecameFalse,
    Unknown(u32),
}
/// The two predicate edges supported by the native key-B queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkPredicateEdge {
    BecameTrue,
    BecameFalse,
}
impl ThinkPredicateEdge {
    fn tag(self) -> u32 {
        match self {
            Self::BecameTrue => 1,
            Self::BecameFalse => 2,
        }
    }
}
impl ThinkActionEvent {
    pub fn predicate_transition(&self) -> ThinkPredicateTransition {
        match self.raw_tag {
            0 => ThinkPredicateTransition::NoChange,
            1 => ThinkPredicateTransition::BecameTrue,
            2 => ThinkPredicateTransition::BecameFalse,
            tag => ThinkPredicateTransition::Unknown(tag),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinkActionQueue {
    pub context_address: u64,
    pub think_address: u64,
    pub interface_address: u64,
    pub queue_address: u64,
    pub allocator_address: u64,
    pub sentinel_address: u64,
    pub events: Vec<ThinkActionEvent>,
}
impl ThinkActionQueue {
    /// Matches the native tag/key-B test. Key A is deliberately not filtered.
    /// Searches only this copied observation; it does not query the engine again.
    pub fn has_transition_for_key_b(&self, edge: ThinkPredicateEdge, key_b: u32) -> bool {
        self.events
            .iter()
            .any(|event| event.raw_tag == edge.tag() && event.raw_key_b == key_b)
    }
    /// Returns the first matching key B in native list order, including zero.
    pub fn first_transition_key_b(&self, edge: ThinkPredicateEdge) -> Option<u32> {
        self.events
            .iter()
            .find(|event| event.raw_tag == edge.tag())
            .map(|event| event.raw_key_b)
    }
    /// Contribution ORed into Think's event word by the native queue aggregator.
    /// The complete event word includes other producers and is not reconstructed.
    pub fn predicate_event_mask(&self) -> u64 {
        self.events.iter().fold(0, |mask, event| {
            mask | match event.raw_tag {
                1 => 0x0800_0000_0000,
                2 => 0x1000_0000_0000,
                _ => 0,
            }
        })
    }
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// Uses a validated Goal context's parent plus exact CSAiFunc backlink and queue owner.
    /// Missing parent returns None; partial initialization, malformed lists or changing
    /// observations fail. No discovery from a character pointer or native queue calls.
    pub fn think_action_queue(
        &self,
        context: u64,
        limit: usize,
    ) -> Result<Option<ThinkActionQueue>> {
        self.core().think_action_queue(context, limit)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn think_action_queue(
        &self,
        context: u64,
        limit: usize,
    ) -> Result<Option<ThinkActionQueue>> {
        if !(1..=4096).contains(&limit) {
            return Err(Error::InvalidProfile(
                "NR Think event limit must be 1..=4096",
            ));
        }
        let goal = self.goal_action_state(context)?;
        let think = goal.raw_parent_address;
        if think == 0 {
            if self.goal_action_state(context)? != goal {
                return Err(Error::InvalidProfile("NR Goal parent changed"));
            }
            return Ok(None);
        }
        if !think.is_multiple_of(8) {
            return Err(Error::InvalidProfile("unaligned NR Think parent"));
        }
        let interface = self.u64(think, 8)?;
        self.check_type(interface, &[AI_FUNC_TABLE])?;
        if self.u64(interface, 8)? != think {
            return Err(Error::InvalidProfile("NR Think interface owner differs"));
        }
        let queue = think.checked_add(0xdb70).ok_or(Error::AddressOverflow)?;
        if self.u64(queue, 0)? != think {
            return Err(Error::InvalidProfile("NR Think queue owner differs"));
        }
        let allocator = self.u64(queue, 8)?;
        let sentinel = self.u64(queue, 0x10)?;
        let count = self.u64(queue, 0x18)?;
        if sentinel == 0 || !sentinel.is_multiple_of(8) || count > limit as u64 {
            return Err(Error::InvalidProfile(
                "invalid or excessive NR Think event list",
            ));
        }
        let first = self.u64(sentinel, 0)?;
        let last = self.u64(sentinel, 8)?;
        let mut current = first;
        let mut previous = sentinel;
        let mut seen = BTreeSet::new();
        let mut events = Vec::new();
        let mut observations = Vec::new();
        for _ in 0..count {
            if current == 0
                || current == sentinel
                || !current.is_multiple_of(8)
                || !seen.insert(current)
            {
                return Err(Error::InvalidProfile(
                    "NR Think event list is short, cyclic or unaligned",
                ));
            }
            let mut raw = [0; 40];
            self.memory.read_exact(current, &mut raw)?;
            let word = |at: usize| u64::from_le_bytes(raw[at..at + 8].try_into().expect("record"));
            if word(8) != previous || word(0x10) != think {
                return Err(Error::InvalidProfile(
                    "NR Think event owner/previous link differs",
                ));
            }
            events.push(ThinkActionEvent {
                address: current,
                raw_key_a: u32::from_le_bytes(raw[0x18..0x1c].try_into().expect("key")),
                raw_key_b: u32::from_le_bytes(raw[0x1c..0x20].try_into().expect("key")),
                raw_flag: raw[0x20],
                raw_tag: u32::from_le_bytes(raw[0x24..0x28].try_into().expect("tag")),
            });
            observations.push((current, raw));
            previous = current;
            current = word(0);
        }
        if current != sentinel || previous != last {
            return Err(Error::InvalidProfile(
                "NR Think event count/sentinel links disagree",
            ));
        }
        for (address, raw) in observations {
            let mut repeated = [0; 40];
            self.memory.read_exact(address, &mut repeated)?;
            if repeated != raw {
                return Err(Error::InvalidProfile("NR Think event changed during copy"));
            }
        }
        self.unchanged_u64(sentinel, 0, first)?;
        self.unchanged_u64(sentinel, 8, last)?;
        for (offset, value) in [(0, think), (8, allocator), (0x10, sentinel), (0x18, count)] {
            self.unchanged_u64(queue, offset, value)?;
        }
        self.unchanged_u64(interface, 8, think)?;
        self.unchanged_u64(think, 8, interface)?;
        self.check_type(interface, &[AI_FUNC_TABLE])?;
        if self.goal_action_state(context)? != goal {
            return Err(Error::InvalidProfile("NR Goal/Think association changed"));
        }
        Ok(Some(ThinkActionQueue {
            context_address: context,
            think_address: think,
            interface_address: interface,
            queue_address: queue,
            allocator_address: allocator,
            sentinel_address: sentinel,
            events,
        }))
    }
}
