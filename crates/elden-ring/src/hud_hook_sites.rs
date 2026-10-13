//! Selected owned-consumer hook sites in the fixed current image. No hook installation or ABI authorization.
use crate::Ww171File;
use shikifsrs_runtime::profile::CodeRange;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudInlineHookKind {
    DamageProcessing,
    HitDispatch,
    SummonRange,
    PhysicsPosition,
    BulletSpawn,
}
#[derive(Debug, Clone, Copy)]
pub struct HudInlineHookSite {
    pub kind: HudInlineHookKind,
    pub rva: u32,
    pub evidence: CodeRange,
}
pub(crate) const HUD_HOOK_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x448910,
        length: 0xc03,
    },
    CodeRange {
        rva: 0x449d30,
        length: 0x732,
    },
    CodeRange {
        rva: 0xea152a,
        length: 0xbf,
    },
    CodeRange {
        rva: 0x467c60,
        length: 0x1b7,
    },
    CodeRange {
        rva: 0x3a2cb0,
        length: 0x13c,
    },
];
impl Ww171File<'_> {
    /// File-bound locations only. The caller must independently retain the actual ABI,
    /// trampoline/closures and exclusive installation/operation phase before any native hook.
    /// The summon range span is a proved control-flow fragment, not a complete native function.
    pub fn hud_inline_hook_sites(&self) -> [HudInlineHookSite; 5] {
        [
            HudInlineHookSite {
                kind: HudInlineHookKind::DamageProcessing,
                rva: 0x448910,
                evidence: HUD_HOOK_EVIDENCE[0],
            },
            HudInlineHookSite {
                kind: HudInlineHookKind::HitDispatch,
                rva: 0x449d30,
                evidence: HUD_HOOK_EVIDENCE[1],
            },
            HudInlineHookSite {
                kind: HudInlineHookKind::SummonRange,
                rva: 0xea1583,
                evidence: HUD_HOOK_EVIDENCE[2],
            },
            HudInlineHookSite {
                kind: HudInlineHookKind::PhysicsPosition,
                rva: 0x467df9,
                evidence: HUD_HOOK_EVIDENCE[3],
            },
            HudInlineHookSite {
                kind: HudInlineHookKind::BulletSpawn,
                rva: 0x3a2cb0,
                evidence: HUD_HOOK_EVIDENCE[4],
            },
        ]
    }
}
