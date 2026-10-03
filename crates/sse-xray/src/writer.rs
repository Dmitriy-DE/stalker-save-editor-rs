//! One-copy X-Ray edit preparation. Unknown fields remain untouched.

use sse_core::{Error, Result, SaveBuffer};
use std::collections::HashSet;

use crate::Save;

const MAXIMUM_MONEY: u32 = 2_000_000_000;

/// Reference capability maturity from the C# 1.3.1 capability registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// The field is confirmed by the reference fixtures.
    Verified,
    /// The reference permits this field with experimental status.
    Experimental,
    /// The field is read-only or unsupported.
    Unsupported,
}

/// A mutation kind listed by the X-Ray capability registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Clone an item from an indexed template.
    AddItems,
    /// Change actor money.
    EditMoney,
    /// Change ammunition counts.
    EditStacks,
    /// Delete registry items.
    RemoveItems,
    /// Change equipment durability.
    EditDurability,
    /// Change item placement.
    EditPlacement,
    /// Change the actor faction.
    EditPlayerFaction,
    /// Change faction relations.
    EditRelations,
    /// Change upgrades.
    EditUpgrades,
    /// Transfer items into or out of a stash.
    MoveItems,
}

/// Returns the C# capability maturity for one format and edit kind.
#[must_use]
pub const fn capability(format: crate::Format, kind: ChangeKind) -> Capability {
    use crate::Format::{Cop, CopEe, Cs, CsEe, Soc, SocEe};
    use Capability::{Experimental, Unsupported, Verified};
    use ChangeKind::{
        AddItems, EditDurability, EditMoney, EditPlacement, EditPlayerFaction, EditRelations, EditStacks, EditUpgrades,
        MoveItems, RemoveItems,
    };

    match (format, kind) {
        (Soc | Cs | Cop, AddItems | EditMoney | EditStacks | RemoveItems) => Verified,
        (Soc | Cs | Cop, EditDurability | EditPlacement | EditPlayerFaction | EditRelations | MoveItems) => {
            Experimental
        }
        (Cs | Cop, EditUpgrades) => Experimental,
        (Soc, EditUpgrades) => Unsupported,
        (SocEe | CsEe | CopEe, AddItems | EditMoney | EditStacks | RemoveItems) => Experimental,
        (
            SocEe | CsEe | CopEe,
            EditDurability | EditPlacement | EditPlayerFaction | EditRelations | EditUpgrades | MoveItems,
        ) => Unsupported,
    }
}

/// A confirmed change to one indexed save field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Replace the actor's wallet after checking its recorded old value.
    SetMoney {
        /// The actor registry object id.
        target_object: u16,
        /// The wallet value observed when the edit was prepared.
        old_value: u32,
        /// The requested wallet value.
        new_value: u32,
    },
    /// Replace the count of one confirmed actor-owned ammunition stack.
    SetStack {
        /// The inventory object id.
        target_object: u16,
        /// The count observed when the edit was prepared.
        old_value: u16,
        /// The requested count.
        new_value: u16,
    },
}

/// A bounded set of edits applied to one save image in one pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    changes: Vec<Change>,
}

impl ChangeSet {
    /// Creates a change set from the requested edits.
    #[must_use]
    pub fn new(changes: Vec<Change>) -> Self {
        Self { changes }
    }

    /// Requested edits in caller order.
    #[must_use]
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }
}

/// Applies a supported change set, repacks once, and verifies the unpacked result.
pub fn apply(save: &Save, changes: &ChangeSet) -> Result<SaveBuffer> {
    if changes.changes.is_empty() {
        return Err(Error::Refused("X-Ray change set is empty".to_owned()));
    }
    if changes.changes.len() > 100_000 {
        return Err(Error::Refused("X-Ray change set exceeds 100000 entries".to_owned()));
    }

    let inventory = if changes
        .changes()
        .iter()
        .any(|change| matches!(change, Change::SetStack { .. }))
    {
        Some(save.inventory()?)
    } else {
        None
    };
    let mut seen_money = false;
    let mut seen_stacks = HashSet::new();
    let mut writes = Vec::with_capacity(changes.changes.len().saturating_mul(2));
    for change in changes.changes() {
        let kind = match change {
            Change::SetMoney { .. } => ChangeKind::EditMoney,
            Change::SetStack { .. } => ChangeKind::EditStacks,
        };
        if capability(save.format(), kind) == Capability::Unsupported {
            return Err(Error::Refused(format!(
                "{kind:?} is unsupported for {}",
                save.format().id()
            )));
        }
        match change {
            Change::SetMoney {
                target_object,
                old_value,
                new_value,
            } => {
                if seen_money {
                    return Err(Error::Refused(
                        "X-Ray change set contains duplicate money edits".to_owned(),
                    ));
                }
                seen_money = true;
                if *target_object != save.actor_id() {
                    return Err(Error::Refused("money edit target is not the actor".to_owned()));
                }
                if *old_value != save.money()? {
                    return Err(Error::Refused(
                        "money edit old value does not match the indexed save".to_owned(),
                    ));
                }
                if *new_value > MAXIMUM_MONEY {
                    return Err(Error::Refused(format!("money must be in the range 0..{MAXIMUM_MONEY}")));
                }
                writes.push(PendingWrite::u32(save.money_offset(), *new_value));
            }
            Change::SetStack {
                target_object,
                old_value,
                new_value,
            } => {
                if !seen_stacks.insert(*target_object) {
                    return Err(Error::Refused(format!(
                        "duplicate stack edit for 0x{target_object:04X}"
                    )));
                }
                if *new_value == 0 {
                    return Err(Error::Refused("stack count must be in the range 1..65535".to_owned()));
                }
                let item = inventory
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .find(|item| item.handle == *target_object)
                    .ok_or_else(|| Error::Refused(format!("object 0x{target_object:04X} is not actor-owned")))?;
                if item.count != Some(*old_value) {
                    return Err(Error::Refused(format!(
                        "stack old count does not match object 0x{target_object:04X}"
                    )));
                }
                let state_offset = item.state_count_offset.ok_or_else(|| {
                    Error::Refused(format!("object 0x{target_object:04X} has no proven state count field"))
                })?;
                let update_offset = item.update_count_offset.ok_or_else(|| {
                    Error::Refused(format!("object 0x{target_object:04X} has no proven update count field"))
                })?;
                writes.push(PendingWrite::u16(state_offset, *new_value));
                writes.push(PendingWrite::u16(update_offset, *new_value));
            }
        }
    }
    writes.sort_by_key(|write| write.offset);
    for pair in writes.windows(2) {
        let Some((previous, next)) = pair.first().zip(pair.get(1)) else {
            continue;
        };
        let previous_end = previous
            .offset
            .checked_add(previous.length)
            .ok_or_else(|| Error::damaged("X-Ray write range overflow"))?;
        if previous_end > next.offset {
            return Err(Error::Refused("X-Ray changes overlap the same image bytes".to_owned()));
        }
    }

    let mut working = save.raw_image().to_vec();
    for write in &writes {
        let end = write
            .offset
            .checked_add(write.length)
            .ok_or_else(|| Error::damaged("X-Ray write range overflow"))?;
        let field = working
            .get_mut(write.offset..end)
            .ok_or_else(|| Error::damaged("X-Ray write range is outside the image"))?;
        let value = write
            .bytes
            .get(..write.length)
            .ok_or_else(|| Error::damaged("X-Ray write value has an invalid width"))?;
        field.copy_from_slice(value);
    }

    let packed = save.repack(&working)?;
    let verified = Save::read(packed.as_slice())?;
    if verified.format() != save.format() {
        return Err(Error::Refused("X-Ray edit changed the detected save format".to_owned()));
    }
    let before = save.raw_image();
    let after = verified.raw_image();
    if before.len() != after.len() {
        return Err(Error::Refused(
            "X-Ray edit changed the unpacked image length".to_owned(),
        ));
    }
    let mut unchanged_start = 0_usize;
    for write in &writes {
        let end = write
            .offset
            .checked_add(write.length)
            .ok_or_else(|| Error::damaged("X-Ray write range overflow"))?;
        if before.get(unchanged_start..write.offset) != after.get(unchanged_start..write.offset) {
            return Err(Error::Refused(
                "X-Ray edit changed bytes outside its owned ranges".to_owned(),
            ));
        }
        let actual = after
            .get(write.offset..end)
            .ok_or_else(|| Error::damaged("X-Ray read-back write range is outside the image"))?;
        let expected = write
            .bytes
            .get(..write.length)
            .ok_or_else(|| Error::damaged("X-Ray write value has an invalid width"))?;
        if actual != expected {
            return Err(Error::Refused("X-Ray edit failed its byte read-back check".to_owned()));
        }
        unchanged_start = end;
    }
    if before.get(unchanged_start..) != after.get(unchanged_start..) {
        return Err(Error::Refused(
            "X-Ray edit changed bytes outside its owned ranges".to_owned(),
        ));
    }
    if seen_money {
        let expected_money = changes.changes().iter().find_map(|change| match change {
            Change::SetMoney { new_value, .. } => Some(*new_value),
            Change::SetStack { .. } => None,
        });
        if verified.money()? != expected_money.unwrap_or_default() {
            return Err(Error::Refused(
                "X-Ray money edit failed its value read-back check".to_owned(),
            ));
        }
    }
    if !seen_stacks.is_empty() {
        let verified_items = verified.inventory()?;
        for change in changes.changes() {
            if let Change::SetStack {
                target_object,
                new_value,
                ..
            } = change
            {
                let item = verified_items
                    .iter()
                    .find(|item| item.handle == *target_object)
                    .ok_or_else(|| Error::Refused(format!("stack object 0x{target_object:04X} disappeared")))?;
                if item.count != Some(*new_value) {
                    return Err(Error::Refused(format!(
                        "stack count read-back failed for 0x{target_object:04X}"
                    )));
                }
            }
        }
    }
    Ok(packed)
}

#[derive(Debug)]
struct PendingWrite {
    offset: usize,
    bytes: [u8; 4],
    length: usize,
}

impl PendingWrite {
    fn u16(offset: usize, value: u16) -> Self {
        let [first, second] = value.to_le_bytes();
        Self {
            offset,
            bytes: [first, second, 0, 0],
            length: 2,
        }
    }

    fn u32(offset: usize, value: u32) -> Self {
        Self {
            offset,
            bytes: value.to_le_bytes(),
            length: 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{apply, capability, Capability, Change, ChangeKind, ChangeSet};
    use crate::{Format, Save};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn capability_maturities_match_the_reference_matrix() {
        for format in [Format::Soc, Format::Cs, Format::Cop] {
            for kind in [
                ChangeKind::AddItems,
                ChangeKind::EditMoney,
                ChangeKind::EditStacks,
                ChangeKind::RemoveItems,
            ] {
                assert_eq!(capability(format, kind), Capability::Verified);
            }
            for kind in [
                ChangeKind::EditDurability,
                ChangeKind::EditPlacement,
                ChangeKind::EditPlayerFaction,
                ChangeKind::EditRelations,
                ChangeKind::MoveItems,
            ] {
                assert_eq!(capability(format, kind), Capability::Experimental);
            }
        }
        assert_eq!(
            capability(Format::Soc, ChangeKind::EditUpgrades),
            Capability::Unsupported
        );
        for format in [Format::Cs, Format::Cop] {
            assert_eq!(capability(format, ChangeKind::EditUpgrades), Capability::Experimental);
        }
        for format in [Format::SocEe, Format::CsEe, Format::CopEe] {
            for kind in [
                ChangeKind::AddItems,
                ChangeKind::EditMoney,
                ChangeKind::EditStacks,
                ChangeKind::RemoveItems,
            ] {
                assert_eq!(capability(format, kind), Capability::Experimental);
            }
            for kind in [
                ChangeKind::EditDurability,
                ChangeKind::EditPlacement,
                ChangeKind::EditPlayerFaction,
                ChangeKind::EditRelations,
                ChangeKind::EditUpgrades,
                ChangeKind::MoveItems,
            ] {
                assert_eq!(capability(format, kind), Capability::Unsupported);
            }
        }
    }

    #[test]
    fn money_changes_match_all_six_reference_packed_and_raw_fixtures() -> TestResult {
        let pairs: [(&[u8], &[u8], &[u8]); 6] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cs-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cs-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cs-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-ee-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cs-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cs-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cs-ee-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-ee-expected.raw"),
            ),
        ];

        for (source_bytes, expected_packed, expected_raw) in pairs {
            let source = Save::read(source_bytes)?;
            let expected = Save::read(expected_packed)?;
            let actor_id = source
                .registry_objects()
                .iter()
                .find(|object| object.name.eq_ignore_ascii_case("actor"))
                .ok_or_else(|| std::io::Error::other("actor object should be indexed"))?
                .object_id;
            let changes = ChangeSet::new(vec![Change::SetMoney {
                target_object: actor_id,
                old_value: source.money()?,
                new_value: expected.money()?,
            }]);

            let output = apply(&source, &changes)?;
            assert_eq!(output.as_slice(), expected_packed);
            let read_back = Save::read(output.as_slice())?;
            assert_eq!(read_back.raw_image(), expected_raw);

            let inverse = ChangeSet::new(vec![Change::SetMoney {
                target_object: actor_id,
                old_value: expected.money()?,
                new_value: source.money()?,
            }]);
            let restored = apply(&read_back, &inverse)?;
            assert_eq!(restored.as_slice(), source_bytes);
        }
        Ok(())
    }

    #[test]
    fn refuses_a_change_whose_old_value_does_not_match() -> TestResult {
        let source = Save::read(include_bytes!("../../../fixtures/synthetic/xray-soc.sav"))?;
        let actor_id = source
            .registry_objects()
            .iter()
            .find(|object| object.name.eq_ignore_ascii_case("actor"))
            .ok_or_else(|| std::io::Error::other("actor object should be indexed"))?
            .object_id;
        let changes = ChangeSet::new(vec![Change::SetMoney {
            target_object: actor_id,
            old_value: source
                .money()?
                .checked_add(1)
                .ok_or_else(|| std::io::Error::other("money test value overflow"))?,
            new_value: 50,
        }]);

        assert!(apply(&source, &changes).is_err());
        Ok(())
    }

    #[test]
    fn stack_changes_match_all_six_reference_packed_and_raw_fixtures() -> TestResult {
        let pairs: [(&[u8], &[u8], &[u8]); 6] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cs-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cs-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cs-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cop-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cop-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cop-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-ee-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cs-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cs-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cs-ee-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cop-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cop-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cop-ee-expected.raw"),
            ),
        ];

        for (source_bytes, expected_packed, expected_raw) in pairs {
            let source = Save::read(source_bytes)?;
            let expected = Save::read(expected_packed)?;
            assert_eq!(expected.raw_image(), expected_raw);
            let original_count = source
                .inventory()?
                .into_iter()
                .find(|item| item.handle == 0x1234)
                .and_then(|item| item.count)
                .ok_or_else(|| std::io::Error::other("ammunition count should be indexed"))?;
            let changes = ChangeSet::new(vec![Change::SetStack {
                target_object: 0x1234,
                old_value: original_count,
                new_value: 44,
            }]);

            let output = apply(&source, &changes)?;
            assert_eq!(output.as_slice(), expected_packed);
            let read_back = Save::read(output.as_slice())?;
            assert_eq!(read_back.raw_image(), expected_raw);

            let inverse = ChangeSet::new(vec![Change::SetStack {
                target_object: 0x1234,
                old_value: 44,
                new_value: original_count,
            }]);
            let restored = apply(&read_back, &inverse)?;
            assert_eq!(restored.as_slice(), source_bytes);
        }
        Ok(())
    }
}
